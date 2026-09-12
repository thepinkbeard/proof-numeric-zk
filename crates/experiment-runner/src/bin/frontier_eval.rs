//! Cobertura normalizada das representações da fronteira de Pareto.
//!
//! Os avaliadores anteriores emitiam três esquemas incompatíveis, cada um com
//! um subconjunto das representações: `experiment-runner` sem CFD, `cfd_eval`
//! só com CFD, `workload_eval` só com CBD. Nenhum deles carregava chave de
//! junção com o lado de custo.
//!
//! Este binário roda o mesmo workload em todas as representações da matriz
//! congelada em `docs/pareto-protocol.md`, com um único esquema de saída e o
//! `profile_hash_hex` que junta cobertura e custo.
//!
//! As trajetórias são geradas pelas **testemunhas dos circuitos**, não por uma
//! reimplementação da aritmética. É o que garante que a cobertura descreva a
//! mesma computação cujo custo foi medido.

use anyhow::{bail, Context, Result};
use clap::Parser;
use nova_circuit::{
    circuit_decision, common_threshold, initial_cbd_state, initial_compensated_state,
    initial_fixed_point_state, AffineParameters, AffineStepWitness, CbdAffineStepWitness,
    CbdRadiusBound, CbdSequentialStepWitness, CbdStepWitness, CfdLogisticStepWitness,
    CfdStableStepWitness, CfdTopology, CircuitRounding, CompensatedStepWitness, StepWitness,
};
use num_bigint::{BigInt, BigUint};
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};
use numeric_core::{
    CertifiedDyadic, CertifiedFloat, CfdOperation, CfdProfile, Decision3, DecisionAdaptiveProfile,
    FrontierPoint, NumericProfile, PrecisionPolicy, RepresentationId, RoundingId, WorkloadId,
    WorkloadSpec,
};
use numeric_reference::{
    decimal_ratio, logistic_trajectory_high_precision, restricted_logistic_step,
    restricted_stable_step, HighPrecisionConfig,
};
use rug::Float;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Parser)]
#[command(about = "Cobertura normalizada das representações da fronteira de Pareto")]
struct Args {
    #[arg(long, default_value_t = 1000)]
    steps: usize,
    #[arg(long, default_value = "results/frontier_coverage.json")]
    output: PathBuf,
    /// Restringe a varredura a uma representação, pelo rótulo canônico.
    #[arg(long)]
    only: Option<String>,
    #[arg(long, default_value = "results/adult_logistic_model.json")]
    adult_model: PathBuf,
    #[arg(long, default_value_t = 2000)]
    ecef_cases: usize,
}

// ---------------------------------------------------------------------------
// Estados e decisões
// ---------------------------------------------------------------------------

/// Enclosure de um estado. Formatos não certificados têm `lower == upper`.
#[derive(Clone, Debug)]
struct StateView {
    lower: BigRational,
    upper: BigRational,
    wide: bool,
}

/// Additive qualification interface; historical profiles and outputs unchanged.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QualificationState {
    pub lower: BigRational,
    pub upper: BigRational,
}

pub fn qualification_trajectory(
    family: &str, bits: usize, x0: &BigRational, parameter: &BigRational,
    innovation: Option<&BigRational>, steps: usize,
) -> Result<Vec<QualificationState>> {
    if x0 < &BigRational::zero() || x0 > &BigRational::one() || parameter < &BigRational::zero()
        || parameter > &BigRational::from_integer(4.into()) {
        bail!("qualification input/parameter outside domain");
    }
    if let Some(c) = innovation {
        if parameter >= &BigRational::one() || c < &BigRational::zero()
            || c >= &(BigRational::from_integer(2.into()) * (BigRational::one()-parameter)) {
            bail!("affine attractor outside [0,2)");
        }
    }
    let (representation, rounding) = match family {
        "fixed-truncate" => (RepresentationId::Standard, RoundingId::Truncate),
        "fixed-rne" => (RepresentationId::Standard, RoundingId::NearestTiesToEven),
        "cbd-local" => (RepresentationId::CbdLocalTaylor, RoundingId::NearestTiesToEven),
        "cbd-sequential" if innovation.is_none() => (RepresentationId::CbdSequentialLocalTaylor, RoundingId::NearestTiesToEven),
        "cfd" => (RepresentationId::CfdSequential, RoundingId::NearestTiesToEven),
        "fp32" => (RepresentationId::Fp32Exact, RoundingId::NearestTiesToEven),
        _ => bail!("unsupported qualification family {family}"),
    };
    if !([16, 32, 48, 64].contains(&bits) || bits == 24 && ["cfd", "fp32"].contains(&family)) {
        bail!("unsupported precision");
    }
    let mut variant = Variant::plain(representation, bits, rounding);
    if family == "cfd" {
        variant.exponent_lanes = if innovation.is_some() { (-24..=1).collect() } else { (-16..=0).collect() };
        variant.radius_bits = 16;
    }
    let trajectory = if let Some(c) = innovation {
        affine_trajectory_for(&variant, x0, parameter, c, AffineParameters {
            alpha_num: parameter.numer().to_u64().context("alpha numerator")?,
            alpha_den: parameter.denom().to_u64().context("alpha denominator")?,
            innovation_num: c.numer().to_u64().context("innovation numerator")?,
            innovation_den: c.denom().to_u64().context("innovation denominator")?,
        }, steps, &[])?
    } else { trajectory_for(&variant, x0, parameter, steps, &[])? };
    Ok(trajectory.states.into_iter().map(|s| QualificationState { lower:s.lower, upper:s.upper }).collect())
}

impl StateView {
    fn exact(value: BigRational) -> Self {
        Self {
            lower: value.clone(),
            upper: value,
            wide: false,
        }
    }

    fn width(&self) -> f64 {
        (&self.upper - &self.lower)
            .to_f64()
            .unwrap_or(f64::INFINITY)
    }

    /// Regra de decisão do intervalo, idêntica à do gadget CBD: a decisão só é
    /// definitiva quando o enclosure inteiro fica de um lado do limiar.
    fn decide(&self, threshold: &BigRational) -> Decision3 {
        if self.lower >= *threshold {
            Decision3::True
        } else if self.upper < *threshold {
            Decision3::False
        } else {
            Decision3::Indeterminate
        }
    }
}

struct Trajectory {
    states: Vec<StateView>,
    /// Decisões por limiar, na ordem de `thresholds`.
    decisions: Vec<Vec<Decision3>>,
}

fn decide_all(states: &[StateView], thresholds: &[BigRational]) -> Vec<Vec<Decision3>> {
    thresholds
        .iter()
        .map(|threshold| states.iter().map(|state| state.decide(threshold)).collect())
        .collect()
}

// ---------------------------------------------------------------------------
// Referência
// ---------------------------------------------------------------------------

/// Referência MPFR com bound conservador, já resolvida por limiar.
///
/// As decisões são pré-computadas uma vez por `r`: os intervalos da referência
/// têm milhares de bits e recomparar por configuração dominaria o tempo.
struct Reference {
    intervals: Vec<(BigRational, BigRational)>,
    /// `outcomes[limiar][estado]`; `None` quando o intervalo da própria
    /// referência cruza o limiar e a oportunidade não pode ser julgada.
    outcomes: Vec<Vec<Option<bool>>>,
}

fn float_to_rational(value: &Float) -> BigRational {
    let (mantissa, exponent) = value
        .to_integer_exp()
        .expect("referência MPFR é sempre finita");
    let hexadecimal = mantissa.to_string_radix(16);
    let negative = hexadecimal.starts_with('-');
    let magnitude = BigInt::parse_bytes(hexadecimal.trim_start_matches('-').as_bytes(), 16)
        .expect("mantissa hexadecimal válida");
    let signed = if negative { -magnitude } else { magnitude };
    if exponent >= 0 {
        BigRational::from_integer(signed << exponent as usize)
    } else {
        BigRational::new(signed, BigInt::one() << (-exponent) as usize)
    }
}

fn logistic_reference(
    x0: &BigRational,
    r: &BigRational,
    steps: usize,
    thresholds: &[BigRational],
) -> Result<Reference> {
    let precision_bits = (2 * steps + 128).max(256) as u32;
    let trajectory =
        logistic_trajectory_high_precision(x0, r, steps, &HighPrecisionConfig { precision_bits })?;
    let intervals: Vec<(BigRational, BigRational)> = trajectory
        .iter()
        .map(|point| {
            let mut lower = Float::with_val(precision_bits, &point.value);
            lower -= &point.estimated_absolute_error;
            let mut upper = Float::with_val(precision_bits, &point.value);
            upper += &point.estimated_absolute_error;
            (float_to_rational(&lower), float_to_rational(&upper))
        })
        .collect();
    let outcomes = thresholds
        .iter()
        .map(|threshold| {
            intervals
                .iter()
                .map(|(lower, upper)| {
                    if lower >= threshold {
                        Some(true)
                    } else if upper < threshold {
                        Some(false)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect();
    Ok(Reference {
        intervals,
        outcomes,
    })
}

// ---------------------------------------------------------------------------
// Trajetórias por família
// ---------------------------------------------------------------------------

fn limb_numerator<const K: usize>(limbs: [u64; K], is_one: bool) -> u128 {
    if is_one {
        1_u128 << (16 * K)
    } else {
        limbs
            .iter()
            .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
    }
}

fn dyadic(numerator: u128, fractional_bits: usize) -> BigRational {
    BigRational::new(BigInt::from(numerator), BigInt::one() << fractional_bits)
}

fn fixed_point_trajectory<const K: usize>(
    x0: &BigRational,
    r_num: u64,
    r_den: u64,
    steps: usize,
    rounding: CircuitRounding,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let (mut limbs, mut is_one) = initial_fixed_point_state::<K>(x0, rounding);
    let mut states = Vec::with_capacity(steps + 1);
    for index in 0..=steps {
        states.push(StateView::exact(dyadic(
            limb_numerator::<K>(limbs, is_one),
            16 * K,
        )));
        if index < steps {
            let witness = StepWitness::compute_with_rounding(limbs, is_one, r_num, r_den, rounding);
            limbs = witness.output_limbs;
            is_one = witness.output_is_one;
        }
    }
    let decisions = decide_all(&states, thresholds);
    Ok(Trajectory { states, decisions })
}

fn compensated_fine_numerator(limbs: [u64; 2], is_one: bool, residual_offset: u64) -> u128 {
    let scale = 1_u128 << 32;
    let main = limb_numerator::<2>(limbs, is_one);
    main * scale + residual_offset as u128 - scale / 2
}

fn compensated_trajectory(
    x0: &BigRational,
    r_num: u64,
    r_den: u64,
    steps: usize,
    rounding: CircuitRounding,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let (mut limbs, mut is_one, mut residual_offset) = initial_compensated_state(x0, rounding);
    let mut states = Vec::with_capacity(steps + 1);
    for index in 0..=steps {
        // A representação compensada vale na grade fina `2^64`, e é lá que o
        // circuito decide; usar apenas `V` descartaria o residual.
        states.push(StateView::exact(dyadic(
            compensated_fine_numerator(limbs, is_one, residual_offset),
            64,
        )));
        if index < steps {
            let witness = CompensatedStepWitness::compute_with_rounding(
                limbs,
                is_one,
                residual_offset,
                r_num,
                r_den,
                rounding,
            );
            limbs = witness.output_limbs;
            is_one = witness.output_is_one;
            residual_offset = witness.output_residual_offset;
        }
    }
    let decisions = decide_all(&states, thresholds);
    Ok(Trajectory { states, decisions })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CbdMode {
    Global,
    LocalTaylor,
    Sequential,
}

fn cbd_interval<const K: usize>(center: u128, radius: u128, wide: bool) -> StateView {
    let scale = BigInt::one() << (16 * K);
    let center = BigInt::from(center);
    let radius = BigInt::from(radius);
    StateView {
        lower: BigRational::new(&center - &radius, scale.clone()),
        upper: BigRational::new(center + radius, scale),
        wide,
    }
}

fn cbd_trajectory<const K: usize>(
    x0: &BigRational,
    r_num: u64,
    r_den: u64,
    steps: usize,
    mode: CbdMode,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let (mut limbs, mut is_one, mut radius) = initial_cbd_state::<K>(x0);
    let mut capped = false;
    let mut states = Vec::with_capacity(steps + 1);
    for index in 0..=steps {
        states.push(cbd_interval::<K>(
            limb_numerator::<K>(limbs, is_one),
            radius,
            capped,
        ));
        if index < steps {
            match mode {
                CbdMode::Sequential => {
                    let witness =
                        CbdSequentialStepWitness::compute(limbs, is_one, radius, r_num, r_den);
                    limbs = witness.output_limbs;
                    is_one = witness.output_is_one;
                    radius = witness.output_radius;
                    capped = witness.output_radius_capped;
                }
                CbdMode::Global | CbdMode::LocalTaylor => {
                    let bound = if mode == CbdMode::Global {
                        CbdRadiusBound::Global
                    } else {
                        CbdRadiusBound::LocalTaylor
                    };
                    let witness = CbdStepWitness::compute_with_threshold(
                        limbs,
                        is_one,
                        radius,
                        r_num,
                        r_den,
                        bound,
                        1_u128 << (16 * K - 1),
                    );
                    limbs = witness.output_limbs;
                    is_one = witness.output_is_one;
                    radius = witness.output_radius;
                    capped = witness.radius_capped;
                }
            }
        }
    }
    let decisions = decide_all(&states, thresholds);
    Ok(Trajectory { states, decisions })
}

fn cfd_logistic_profile(
    precision_bits: usize,
    lanes: &[i16],
    radius_bits: usize,
    thresholds: &[BigRational],
) -> Result<CfdProfile> {
    CfdProfile::new(
        format!(
            "cfd-unit-p{precision_bits}-e{}_{}-r{radius_bits}",
            lanes.first().copied().unwrap_or_default(),
            lanes.last().copied().unwrap_or_default()
        ),
        1,
        precision_bits,
        lanes.to_vec(),
        radius_bits,
        BigRational::zero(),
        BigRational::one(),
        vec![
            CfdOperation::Add,
            CfdOperation::Multiply,
            CfdOperation::OneMinus,
            CfdOperation::Fma,
            CfdOperation::LogisticSequential,
            CfdOperation::LogisticFused,
        ],
        thresholds.to_vec(),
        None,
    )
    .context("perfil CFD inválido")
}

fn cfd_state(
    profile: &CfdProfile,
    value: &nova_circuit::CfdCircuitValue,
) -> Result<CertifiedFloat> {
    CertifiedFloat::from_parts(
        profile.clone(),
        false,
        value.exponent_lane as usize,
        BigUint::from(value.significand),
        BigUint::from(value.radius),
        value.zero,
        value.wide,
    )
    .context("estado CFD circuital canônico")
}

fn cfd_trajectory(
    profile: &CfdProfile,
    x0: &BigRational,
    r_num: u64,
    r_den: u64,
    steps: usize,
    topology: CfdTopology,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    // A CFD decide na escala diádica comum do perfil, não pelo intervalo
    // racional; usar a regra genérica divergiria do que o circuito prova.
    let common: Vec<u128> = thresholds
        .iter()
        .map(|threshold| {
            common_threshold(profile, threshold)
                .with_context(|| format!("limiar {threshold} fora da escala comum do perfil CFD"))
        })
        .collect::<Result<_>>()?;

    let mut value =
        CertifiedFloat::from_exact(profile.clone(), x0).context("x0 não pertence ao perfil CFD")?;
    let mut states = Vec::with_capacity(steps + 1);
    let mut decisions = vec![Vec::with_capacity(steps + 1); thresholds.len()];
    for index in 0..=steps {
        states.push(StateView {
            lower: value.lower(),
            upper: value.upper(),
            wide: value.is_wide(),
        });
        for (slot, threshold) in decisions.iter_mut().zip(&common) {
            slot.push(circuit_decision(profile, &value, *threshold)?);
        }
        if index < steps {
            let witness = CfdLogisticStepWitness::compute(profile, &value, r_num, r_den, topology)
                .with_context(|| format!("testemunha CFD na iteração {index}"))?;
            value = cfd_state(profile, &witness.output)?;
        }
    }
    Ok(Trajectory { states, decisions })
}

fn fp32_trajectory(
    x0: &BigRational,
    r: &BigRational,
    steps: usize,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let parameter_bits = rational_to_f32(r, "r")?.to_bits();
    let mut input_bits = rational_to_f32(x0, "x0")?.to_bits();
    let mut states = Vec::with_capacity(steps + 1);
    for index in 0..=steps {
        let value = f32::from_bits(input_bits);
        states.push(StateView::exact(
            BigRational::from_float(value as f64).context("valor FP32 finito")?,
        ));
        if index < steps {
            let trace = restricted_logistic_step(input_bits, parameter_bits)
                .with_context(|| format!("passo FP32 restrito na iteração {index}"))?;
            input_bits = trace.output.bits();
        }
    }
    let decisions = decide_all(&states, thresholds);
    Ok(Trajectory { states, decisions })
}

fn cdad_logistic_trajectories(
    x0: &BigRational,
    r: &BigRational,
    steps: usize,
    thresholds: &[BigRational],
    policy: PrecisionPolicy,
) -> Result<Vec<(Trajectory, AdaptiveMetrics)>> {
    let profile = DecisionAdaptiveProfile::new(
        format!("cdad-logistic-{}", policy.tag()),
        1,
        vec![16, 32, 48, 64],
        BigRational::zero(),
        BigRational::one(),
        thresholds.to_vec(),
        policy,
        numeric_core::CommitmentPolicy::ExistingPoseidonLink,
    )
    .context("perfil CDAD logístico")?;
    let mut by_precision = BTreeMap::<usize, Vec<CertifiedDyadic>>::new();
    for bits in profile.precision_ladder() {
        let numeric = profile.numeric_profile(*bits)?;
        let mut value = CertifiedDyadic::from_exact(numeric, x0)?;
        let mut values = Vec::with_capacity(steps + 1);
        for index in 0..=steps {
            values.push(value.clone());
            if index < steps {
                value = value.logistic_step_local_bound(r)?;
            }
        }
        by_precision.insert(*bits, values);
    }

    thresholds
        .iter()
        .map(|threshold| {
            let mut states = Vec::with_capacity(steps + 1);
            let mut decisions = Vec::with_capacity(steps + 1);
            let mut metrics = AdaptiveMetrics::default();
            for index in 0..=steps {
                let adaptive = profile.evaluate(threshold, |numeric| {
                    Ok(by_precision[&numeric.fractional_bits()][index].clone())
                })?;
                metrics.observe(&adaptive, profile.precision_ladder());
                states.push(StateView {
                    lower: adaptive.value.lower(),
                    upper: adaptive.value.upper(),
                    wide: false,
                });
                decisions.push(adaptive.decision);
            }
            Ok((
                Trajectory {
                    states,
                    decisions: vec![decisions],
                },
                metrics,
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Recorrência afim estável
// ---------------------------------------------------------------------------

/// Referência MPFR da recorrência `x' = alpha*x + c`.
///
/// A recorrência é contrativa, então o erro de arredondamento da referência
/// também contrai em vez de se acumular: o bound é `eps * (1 + alpha + alpha^2 +
/// ...) < eps/(1-alpha)`. Ao contrário do mapa logístico, não há amplificação a
/// compensar, e uma precisão fixa basta para milhares de passos.
fn affine_reference(
    x0: &BigRational,
    alpha: &BigRational,
    innovation: &BigRational,
    steps: usize,
    thresholds: &[BigRational],
) -> Result<Reference> {
    // A distância ao atrator cai como `alpha^n`, então resolver o passo `n` exige
    // cerca de `n` bits. Com precisão fixa, a cauda da trajetória cairia dentro
    // do próprio erro da referência e viraria `reference_inconclusive` — que é o
    // regime em que a comparação mais interessa, porque é onde a decisão é
    // marginal.
    let precision_bits = (steps as u32 + 256).max(512);
    let alpha_float = rational_to_float(alpha, precision_bits);
    let innovation_float = rational_to_float(innovation, precision_bits);
    let mut value = rational_to_float(x0, precision_bits);
    // Erro por passo: uma ULP relativa da mantissa, propagada por `alpha` e
    // somada à nova. O denominador `1 - alpha` fecha a série.
    let unit = Float::with_val(precision_bits, 1) >> (precision_bits - 1);
    let alpha_upper = alpha.to_f64().unwrap_or(1.0);
    let error_bound = if alpha_upper < 1.0 {
        Float::with_val(precision_bits, 4) * &unit / (1.0 - alpha_upper)
    } else {
        Float::with_val(precision_bits, steps as u32) * &unit
    };

    let mut intervals = Vec::with_capacity(steps + 1);
    for index in 0..=steps {
        let mut lower = Float::with_val(precision_bits, &value);
        lower -= &error_bound;
        let mut upper = Float::with_val(precision_bits, &value);
        upper += &error_bound;
        intervals.push((float_to_rational(&lower), float_to_rational(&upper)));
        if index < steps {
            value = value * &alpha_float + &innovation_float;
        }
    }
    let outcomes = thresholds
        .iter()
        .map(|threshold| {
            intervals
                .iter()
                .map(|(lower, upper)| {
                    if lower >= threshold {
                        Some(true)
                    } else if upper < threshold {
                        Some(false)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect();
    Ok(Reference {
        intervals,
        outcomes,
    })
}

fn rational_to_float(value: &BigRational, precision_bits: u32) -> Float {
    let part = |integer: &BigInt| {
        Float::with_val(
            precision_bits,
            rug::Integer::from_str_radix(&integer.to_str_radix(16), 16)
                .expect("inteiro hexadecimal válido"),
        )
    };
    part(value.numer()) / part(value.denom())
}

/// Estado afim `(limbs, alto)`: o valor é `alto + N/S`, cobrindo `[0,2)`.
fn affine_state_numerator<const K: usize>(limbs: [u64; K], high: bool) -> u128 {
    limbs
        .iter()
        .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
        + if high { 1_u128 << (16 * K) } else { 0 }
}

fn affine_initial_limbs<const K: usize>(x0: &BigRational) -> ([u64; K], bool, bool) {
    let scale = 1_u128 << (16 * K);
    let scaled = x0 * BigRational::from_integer(BigInt::from(scale));
    let exact = scaled.is_integer();
    let value = (scaled.numer() / scaled.denom())
        .to_u128()
        .expect("x0 dentro da faixa");
    let high = value >= scale;
    let mut remaining = value - if high { scale } else { 0 };
    let mut limbs = [0_u64; K];
    for limb in limbs.iter_mut().rev() {
        *limb = (remaining & 0xffff) as u64;
        remaining >>= 16;
    }
    (limbs, high, exact)
}

fn affine_fixed_point_trajectory<const K: usize>(
    x0: &BigRational,
    parameters: AffineParameters,
    steps: usize,
    rounding: CircuitRounding,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let (mut limbs, mut high, _) = affine_initial_limbs::<K>(x0);
    let mut states = Vec::with_capacity(steps + 1);
    for index in 0..=steps {
        states.push(StateView::exact(dyadic(
            affine_state_numerator::<K>(limbs, high),
            16 * K,
        )));
        if index < steps {
            let witness = AffineStepWitness::<K>::compute(limbs, high, parameters, rounding);
            limbs = witness.output_limbs;
            high = witness.output_high;
        }
    }
    let decisions = decide_all(&states, thresholds);
    Ok(Trajectory { states, decisions })
}

fn affine_cbd_trajectory<const K: usize>(
    x0: &BigRational,
    parameters: AffineParameters,
    steps: usize,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let fractional_bits = 16 * K;
    let (mut limbs, mut high, exact) = affine_initial_limbs::<K>(x0);
    let mut radius = u128::from(!exact);
    // O limiar da testemunha só existe para preencher `decision`; a decisão
    // reportada vem do enclosure, igual às demais famílias certificadas.
    let nominal_threshold = 1_u128 << fractional_bits;
    let mut states = Vec::with_capacity(steps + 1);
    for index in 0..=steps {
        let center = affine_state_numerator::<K>(limbs, high);
        states.push(StateView {
            lower: dyadic(center.saturating_sub(radius), fractional_bits),
            upper: dyadic(center + radius, fractional_bits),
            wide: false,
        });
        if index < steps {
            let witness = CbdAffineStepWitness::<K>::compute(
                limbs,
                high,
                radius,
                parameters,
                CircuitRounding::NearestTiesToEven,
                nominal_threshold,
            );
            limbs = witness.output_limbs;
            high = witness.output_high;
            radius = witness.output_radius;
        }
    }
    let decisions = decide_all(&states, thresholds);
    Ok(Trajectory { states, decisions })
}

fn cfd_affine_profile(
    precision_bits: usize,
    lanes: &[i16],
    radius_bits: usize,
    thresholds: &[BigRational],
) -> Result<CfdProfile> {
    CfdProfile::new(
        format!(
            "cfd-affine-p{precision_bits}-e{}_{}-r{radius_bits}",
            lanes.first().copied().unwrap_or_default(),
            lanes.last().copied().unwrap_or_default()
        ),
        1,
        precision_bits,
        lanes.to_vec(),
        radius_bits,
        BigRational::zero(),
        BigRational::from_integer(2.into()),
        vec![
            CfdOperation::Add,
            CfdOperation::Multiply,
            CfdOperation::Fma,
            CfdOperation::StableAffine,
        ],
        thresholds.to_vec(),
        None,
    )
    .context("perfil CFD afim inválido")
}

fn cfd_affine_trajectory(
    profile: &CfdProfile,
    x0: &BigRational,
    parameters: AffineParameters,
    steps: usize,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let common: Vec<u128> = thresholds
        .iter()
        .map(|threshold| {
            common_threshold(profile, threshold).with_context(|| {
                format!("limiar {threshold} fora da escala comum do perfil CFD afim")
            })
        })
        .collect::<Result<_>>()?;
    let mut value = CertifiedFloat::from_exact(profile.clone(), x0)
        .context("x0 não pertence ao perfil CFD afim")?;
    let mut states = Vec::with_capacity(steps + 1);
    let mut decisions = vec![Vec::with_capacity(steps + 1); thresholds.len()];
    for index in 0..=steps {
        states.push(StateView {
            lower: value.lower(),
            upper: value.upper(),
            wide: value.is_wide(),
        });
        for (slot, threshold) in decisions.iter_mut().zip(&common) {
            slot.push(circuit_decision(profile, &value, *threshold)?);
        }
        if index < steps {
            let witness = CfdStableStepWitness::compute(
                profile,
                &value,
                parameters.alpha_num,
                parameters.alpha_den,
                parameters.innovation_num,
                parameters.innovation_den,
            )
            .with_context(|| format!("testemunha CFD afim na iteração {index}"))?;
            value = cfd_state(profile, &witness.output)?;
        }
    }
    Ok(Trajectory { states, decisions })
}

fn fp32_affine_trajectory(
    x0: &BigRational,
    alpha: &BigRational,
    innovation: &BigRational,
    steps: usize,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let alpha_bits = rational_to_f32(alpha, "alpha")?.to_bits();
    let innovation_bits = rational_to_f32(innovation, "innovation")?.to_bits();
    let mut input_bits = rational_to_f32(x0, "x0")?.to_bits();
    let mut states = Vec::with_capacity(steps + 1);
    for index in 0..=steps {
        let value = f32::from_bits(input_bits);
        states.push(StateView::exact(
            BigRational::from_float(value as f64).context("valor FP32 finito")?,
        ));
        if index < steps {
            let trace = restricted_stable_step(input_bits, alpha_bits, innovation_bits)
                .with_context(|| format!("passo FP32 afim na iteração {index}"))?;
            input_bits = trace.output.bits();
        }
    }
    let decisions = decide_all(&states, thresholds);
    Ok(Trajectory { states, decisions })
}

fn cdad_affine_trajectories(
    x0: &BigRational,
    alpha: &BigRational,
    innovation: &BigRational,
    steps: usize,
    thresholds: &[BigRational],
    policy: PrecisionPolicy,
) -> Result<Vec<(Trajectory, AdaptiveMetrics)>> {
    let profile = DecisionAdaptiveProfile::new(
        format!("cdad-affine-{}", policy.tag()),
        1,
        vec![16, 32, 48, 64],
        BigRational::zero(),
        BigRational::from_integer(2.into()),
        thresholds.to_vec(),
        policy,
        numeric_core::CommitmentPolicy::ExistingPoseidonLink,
    )
    .context("perfil CDAD afim")?;
    let mut by_precision = BTreeMap::<usize, Vec<CertifiedDyadic>>::new();
    for bits in profile.precision_ladder() {
        let numeric = profile.numeric_profile(*bits)?;
        let addend = CertifiedDyadic::from_exact(numeric.clone(), innovation)?;
        let mut value = CertifiedDyadic::from_exact(numeric, x0)?;
        let mut values = Vec::with_capacity(steps + 1);
        for index in 0..=steps {
            values.push(value.clone());
            if index < steps {
                value = value.mul_public(alpha)?.add(&addend)?;
            }
        }
        by_precision.insert(*bits, values);
    }
    thresholds
        .iter()
        .map(|threshold| {
            let mut states = Vec::with_capacity(steps + 1);
            let mut decisions = Vec::with_capacity(steps + 1);
            let mut metrics = AdaptiveMetrics::default();
            for index in 0..=steps {
                let adaptive = profile.evaluate(threshold, |numeric| {
                    Ok(by_precision[&numeric.fractional_bits()][index].clone())
                })?;
                metrics.observe(&adaptive, profile.precision_ladder());
                states.push(StateView {
                    lower: adaptive.value.lower(),
                    upper: adaptive.value.upper(),
                    wide: false,
                });
                decisions.push(adaptive.decision);
            }
            Ok((
                Trajectory {
                    states,
                    decisions: vec![decisions],
                },
                metrics,
            ))
        })
        .collect()
}

fn rational_to_f32(value: &BigRational, name: &str) -> Result<f32> {
    let converted = value
        .to_f32()
        .with_context(|| format!("--{name} não converte para binary32"))?;
    if !converted.is_finite() {
        bail!("--{name} não é finito em binary32");
    }
    Ok(converted)
}

// ---------------------------------------------------------------------------
// Pontuação
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Counts {
    opportunities: usize,
    definite: usize,
    abstained: usize,
    unsound_definite: usize,
    reference_inconclusive: usize,
    first_abstention: Option<usize>,
    first_wide: Option<usize>,
    enclosure_violations: usize,
    width_sum: f64,
    maximum_width: f64,
}

fn score(
    states: &[StateView],
    decisions: &[Decision3],
    reference: &Reference,
    threshold_index: usize,
    certified: bool,
) -> Counts {
    let mut counts = Counts::default();
    let outcomes = &reference.outcomes[threshold_index];
    for (index, (state, decision)) in states.iter().zip(decisions).enumerate() {
        let width = state.width();
        counts.width_sum += width;
        counts.maximum_width = counts.maximum_width.max(width);
        if state.wide {
            counts.first_wide.get_or_insert(index);
        }
        // Só uma representação certificada afirma conter o valor exato; para as
        // demais, ficar fora do intervalo da referência é o erro esperado da
        // aritmética finita, não uma violação de teorema.
        if certified {
            let (lower, upper) = &reference.intervals[index];
            if state.upper < *lower || state.lower > *upper {
                counts.enclosure_violations += 1;
            }
        }
        let Some(expected) = outcomes[index] else {
            counts.reference_inconclusive += 1;
            continue;
        };
        counts.opportunities += 1;
        match decision {
            Decision3::Indeterminate => {
                counts.abstained += 1;
                counts.first_abstention.get_or_insert(index);
            }
            Decision3::True => {
                counts.definite += 1;
                counts.unsound_definite += usize::from(!expected);
            }
            Decision3::False => {
                counts.definite += 1;
                counts.unsound_definite += usize::from(expected);
            }
        }
    }
    counts
}

// ---------------------------------------------------------------------------
// Matriz e saída
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Variant {
    representation: RepresentationId,
    precision_bits: usize,
    rounding: RoundingId,
    exponent_lanes: Vec<i16>,
    radius_bits: usize,
}

impl Variant {
    fn plain(
        representation: RepresentationId,
        precision_bits: usize,
        rounding: RoundingId,
    ) -> Self {
        Self {
            representation,
            precision_bits,
            rounding,
            exponent_lanes: Vec::new(),
            radius_bits: 0,
        }
    }
}

/// Matriz congelada da seção 5.2 do protocolo, para workloads de trajetória.
fn logistic_variants() -> Vec<Variant> {
    let mut variants = Vec::new();
    for bits in [16, 32, 48, 64] {
        for (rounding, _) in [
            (RoundingId::Truncate, CircuitRounding::Truncate),
            (
                RoundingId::NearestTiesToEven,
                CircuitRounding::NearestTiesToEven,
            ),
        ] {
            variants.push(Variant::plain(RepresentationId::Standard, bits, rounding));
            variants.push(Variant::plain(RepresentationId::Carry, bits, rounding));
        }
    }
    // A representação compensada só existe para `K = 2`; estendê-la é um eixo
    // de custo que o protocolo declara fora de escopo.
    for rounding in [RoundingId::Truncate, RoundingId::NearestTiesToEven] {
        variants.push(Variant::plain(RepresentationId::Compensated, 32, rounding));
    }
    for bits in [16, 32, 48, 64] {
        for representation in [
            RepresentationId::CbdLipschitz,
            RepresentationId::CbdLocalTaylor,
            RepresentationId::CbdSequentialLocalTaylor,
        ] {
            variants.push(Variant::plain(
                representation,
                bits,
                RoundingId::NearestTiesToEven,
            ));
        }
    }
    for precision_bits in [16, 24, 32] {
        for minimum_exponent in [-4_i16, -8, -16] {
            for representation in [RepresentationId::CfdSequential, RepresentationId::CfdFused] {
                variants.push(Variant {
                    representation,
                    precision_bits,
                    rounding: RoundingId::HonestRneCertifiedEnclosure,
                    exponent_lanes: (minimum_exponent..=0).collect(),
                    radius_bits: 16,
                });
            }
        }
    }
    variants.push(Variant::plain(
        RepresentationId::Fp32Exact,
        24,
        RoundingId::NearestTiesToEven,
    ));
    for representation in [
        RepresentationId::CdadFixed,
        RepresentationId::CdadSequential,
        RepresentationId::CdadMarginDirected,
    ] {
        variants.push(Variant::plain(
            representation,
            16,
            RoundingId::NearestTiesToEven,
        ));
    }
    variants
}

macro_rules! by_limbs {
    ($bits:expr, $call:ident, $($arg:expr),* $(,)?) => {
        match $bits {
            16 => $call::<1>($($arg),*),
            32 => $call::<2>($($arg),*),
            48 => $call::<3>($($arg),*),
            64 => $call::<4>($($arg),*),
            other => bail!("precisão {other} não corresponde a 1..=4 limbs de 16 bits"),
        }
    };
}

/// Matriz do workload afim.
///
/// Menor que a do logístico e não por economia. `carry` decompõe o produto de
/// duas variáveis, que a etapa afim não tem, e coincidiria com `standard`. Os
/// três bound modes de CBD coincidem porque `alpha` é o próprio fator de
/// Lipschitz e é exato — não há Taylor a expandir. `compensated` calcula
/// `x*(1-x)` e não tem versão afim. Emitir esses rótulos produziria pontos
/// duplicados na fronteira, com custo idêntico e cobertura idêntica.
fn affine_variants() -> Vec<Variant> {
    let mut variants = Vec::new();
    for bits in [16, 32, 48, 64] {
        for rounding in [RoundingId::Truncate, RoundingId::NearestTiesToEven] {
            variants.push(Variant::plain(RepresentationId::Standard, bits, rounding));
        }
        variants.push(Variant::plain(
            RepresentationId::CbdLocalTaylor,
            bits,
            RoundingId::NearestTiesToEven,
        ));
    }
    for precision_bits in [16, 24, 32] {
        for minimum_exponent in [-16_i16, -24] {
            variants.push(Variant {
                representation: RepresentationId::CfdSequential,
                precision_bits,
                rounding: RoundingId::HonestRneCertifiedEnclosure,
                exponent_lanes: (minimum_exponent..=1).collect(),
                radius_bits: 16,
            });
        }
    }
    variants.push(Variant::plain(
        RepresentationId::Fp32Exact,
        24,
        RoundingId::NearestTiesToEven,
    ));
    for representation in [
        RepresentationId::CdadFixed,
        RepresentationId::CdadSequential,
        RepresentationId::CdadMarginDirected,
    ] {
        variants.push(Variant::plain(
            representation,
            16,
            RoundingId::NearestTiesToEven,
        ));
    }
    variants
}

fn affine_trajectory_for(
    variant: &Variant,
    x0: &BigRational,
    alpha: &BigRational,
    innovation: &BigRational,
    parameters: AffineParameters,
    steps: usize,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let bits = variant.precision_bits;
    match variant.representation {
        RepresentationId::Standard => {
            let rounding = circuit_rounding(variant.rounding)?;
            by_limbs!(
                bits,
                affine_fixed_point_trajectory,
                x0,
                parameters,
                steps,
                rounding,
                thresholds
            )
        }
        RepresentationId::CbdLocalTaylor => {
            by_limbs!(
                bits,
                affine_cbd_trajectory,
                x0,
                parameters,
                steps,
                thresholds
            )
        }
        RepresentationId::CfdSequential => {
            let profile = cfd_affine_profile(
                bits,
                &variant.exponent_lanes,
                variant.radius_bits,
                thresholds,
            )?;
            cfd_affine_trajectory(&profile, x0, parameters, steps, thresholds)
        }
        RepresentationId::Fp32Exact => {
            fp32_affine_trajectory(x0, alpha, innovation, steps, thresholds)
        }
        other => bail!("{} não tem etapa afim; veja affine_variants", other.tag()),
    }
}

fn trajectory_for(
    variant: &Variant,
    x0: &BigRational,
    r: &BigRational,
    steps: usize,
    thresholds: &[BigRational],
) -> Result<Trajectory> {
    let r_num = r.numer().to_u64().context("numerador de r")?;
    let r_den = r.denom().to_u64().context("denominador de r")?;
    let bits = variant.precision_bits;
    match variant.representation {
        RepresentationId::Standard | RepresentationId::Carry => {
            let rounding = circuit_rounding(variant.rounding)?;
            by_limbs!(
                bits,
                fixed_point_trajectory,
                x0,
                r_num,
                r_den,
                steps,
                rounding,
                thresholds
            )
        }
        RepresentationId::Compensated => compensated_trajectory(
            x0,
            r_num,
            r_den,
            steps,
            circuit_rounding(variant.rounding)?,
            thresholds,
        ),
        RepresentationId::CbdLipschitz => {
            by_limbs!(
                bits,
                cbd_trajectory,
                x0,
                r_num,
                r_den,
                steps,
                CbdMode::Global,
                thresholds
            )
        }
        RepresentationId::CbdLocalTaylor => {
            by_limbs!(
                bits,
                cbd_trajectory,
                x0,
                r_num,
                r_den,
                steps,
                CbdMode::LocalTaylor,
                thresholds
            )
        }
        RepresentationId::CbdSequentialLocalTaylor => {
            by_limbs!(
                bits,
                cbd_trajectory,
                x0,
                r_num,
                r_den,
                steps,
                CbdMode::Sequential,
                thresholds
            )
        }
        RepresentationId::CfdSequential | RepresentationId::CfdFused => {
            let profile = cfd_logistic_profile(
                bits,
                &variant.exponent_lanes,
                variant.radius_bits,
                thresholds,
            )?;
            let topology = if variant.representation == RepresentationId::CfdFused {
                CfdTopology::Fused
            } else {
                CfdTopology::Sequential
            };
            cfd_trajectory(&profile, x0, r_num, r_den, steps, topology, thresholds)
        }
        RepresentationId::Fp32Exact => fp32_trajectory(x0, r, steps, thresholds),
        RepresentationId::CdadFixed
        | RepresentationId::CdadSequential
        | RepresentationId::CdadMarginDirected => {
            bail!("CDAD usa o avaliador adaptativo por limiar")
        }
    }
}

fn circuit_rounding(rounding: RoundingId) -> Result<CircuitRounding> {
    match rounding {
        RoundingId::Truncate => Ok(CircuitRounding::Truncate),
        RoundingId::NearestTiesToEven => Ok(CircuitRounding::NearestTiesToEven),
        RoundingId::HonestRneCertifiedEnclosure => {
            bail!("o enclosure certificado não é um modo de arredondamento de ponto fixo")
        }
    }
}

fn rational_text(value: &BigRational) -> String {
    format!("{}/{}", value.numer(), value.denom())
}

#[derive(Debug, Serialize)]
struct CoverageRecord {
    workload: &'static str,
    regime: &'static str,
    representation: &'static str,
    family: &'static str,
    bound_mode: &'static str,
    topology: &'static str,
    rounding: &'static str,
    certified: bool,
    profile_hash_hex: String,
    precision_bits: usize,
    exponent_lanes: Vec<i16>,
    radius_bits: usize,
    parameters: BTreeMap<String, String>,
    threshold: String,
    is_principal_threshold: bool,
    states: usize,
    opportunities: usize,
    definite: usize,
    abstained: usize,
    unsound_definite: usize,
    correct_definite: usize,
    reference_inconclusive: usize,
    availability: Option<f64>,
    unsound_rate: Option<f64>,
    first_abstention: Option<usize>,
    definitive_horizon: Option<usize>,
    first_wide: Option<usize>,
    enclosure_violations: usize,
    mean_interval_width: f64,
    maximum_interval_width: f64,
    initial_precision_bits: Option<usize>,
    mean_final_precision_bits: Option<f64>,
    maximum_final_precision_bits: Option<usize>,
    total_attempts: Option<usize>,
    linked_retries: Option<usize>,
    skipped_rungs: Option<usize>,
    residual_abstentions: Option<usize>,
    attempted_precision_histogram: Option<BTreeMap<usize, usize>>,
}

#[derive(Clone, Debug, Default)]
struct AdaptiveMetrics {
    opportunities: usize,
    final_precision_sum: usize,
    maximum_final_precision: usize,
    total_attempts: usize,
    linked_retries: usize,
    skipped_rungs: usize,
    residual_abstentions: usize,
    attempted_precision_histogram: BTreeMap<usize, usize>,
}

impl AdaptiveMetrics {
    fn observe(&mut self, decision: &numeric_core::AdaptiveDecision, ladder: &[usize]) {
        self.opportunities += 1;
        let final_bits = decision.value.fractional_bits();
        self.final_precision_sum += final_bits;
        self.maximum_final_precision = self.maximum_final_precision.max(final_bits);
        self.total_attempts += decision.attempts.len();
        self.linked_retries += decision.attempts.len().saturating_sub(1);
        self.residual_abstentions += usize::from(decision.decision == Decision3::Indeterminate);
        let attempted_indices: Vec<usize> = decision
            .attempts
            .iter()
            .filter_map(|attempt| {
                ladder
                    .iter()
                    .position(|bits| *bits == attempt.attempted_bits)
            })
            .collect();
        self.skipped_rungs += attempted_indices
            .windows(2)
            .map(|pair| pair[1].saturating_sub(pair[0] + 1))
            .sum::<usize>();
        for attempt in &decision.attempts {
            *self
                .attempted_precision_histogram
                .entry(attempt.attempted_bits)
                .or_default() += 1;
        }
    }
}

#[derive(Debug, Serialize)]
struct Evaluation {
    schema_version: u32,
    experiment: &'static str,
    reference: &'static str,
    steps: usize,
    records: Vec<CoverageRecord>,
}

#[allow(clippy::too_many_arguments)]
fn record(
    variant: &Variant,
    spec: &WorkloadSpec,
    regime: &'static str,
    threshold: &BigRational,
    is_principal: bool,
    states: usize,
    counts: Counts,
    adaptive: Option<&AdaptiveMetrics>,
) -> Result<CoverageRecord> {
    let point = FrontierPoint {
        representation: variant.representation,
        spec: spec.clone(),
        precision_bits: variant.precision_bits,
        exponent_lanes: variant.exponent_lanes.clone(),
        radius_bits: variant.radius_bits,
        rounding: variant.rounding,
        threshold: threshold.clone(),
    };
    let profile = point.profile().context("perfil de experimento inválido")?;
    let correct_definite = counts.definite - counts.unsound_definite;
    let availability =
        (counts.opportunities > 0).then(|| counts.definite as f64 / counts.opportunities as f64);
    let unsound_rate =
        (counts.definite > 0).then(|| counts.unsound_definite as f64 / counts.definite as f64);
    let definitive_horizon = match counts.first_abstention {
        Some(0) => None,
        Some(index) => Some(index - 1),
        None => states.checked_sub(1),
    };
    Ok(CoverageRecord {
        workload: profile.workload().tag(),
        regime,
        representation: variant.representation.tag(),
        family: variant.representation.family().tag(),
        bound_mode: variant.representation.bound_mode().tag(),
        topology: variant.representation.topology().tag(),
        rounding: variant.rounding.tag(),
        certified: variant.representation.is_certified(),
        profile_hash_hex: profile.profile_hash_hex(),
        precision_bits: variant.precision_bits,
        exponent_lanes: variant.exponent_lanes.clone(),
        radius_bits: variant.radius_bits,
        parameters: spec
            .parameters()
            .iter()
            .map(|(name, value)| (name.clone(), rational_text(value)))
            .collect(),
        threshold: rational_text(threshold),
        is_principal_threshold: is_principal,
        states,
        opportunities: counts.opportunities,
        definite: counts.definite,
        abstained: counts.abstained,
        unsound_definite: counts.unsound_definite,
        correct_definite,
        reference_inconclusive: counts.reference_inconclusive,
        availability,
        unsound_rate,
        first_abstention: counts.first_abstention,
        definitive_horizon,
        first_wide: counts.first_wide,
        enclosure_violations: counts.enclosure_violations,
        mean_interval_width: counts.width_sum / states as f64,
        maximum_interval_width: counts.maximum_width,
        initial_precision_bits: adaptive.map(|_| 16),
        mean_final_precision_bits: adaptive.and_then(|metrics| {
            (metrics.opportunities > 0)
                .then(|| metrics.final_precision_sum as f64 / metrics.opportunities as f64)
        }),
        maximum_final_precision_bits: adaptive.map(|metrics| metrics.maximum_final_precision),
        total_attempts: adaptive.map(|metrics| metrics.total_attempts),
        linked_retries: adaptive.map(|metrics| metrics.linked_retries),
        skipped_rungs: adaptive.map(|metrics| metrics.skipped_rungs),
        residual_abstentions: adaptive.map(|metrics| metrics.residual_abstentions),
        attempted_precision_histogram: adaptive
            .map(|metrics| metrics.attempted_precision_histogram.clone()),
    })
}

// ---------------------------------------------------------------------------
// Workloads de decisão única: Adult e ECEF
// ---------------------------------------------------------------------------

/// Um caso de decisão: `viés + Σ aᵢ·bᵢ` comparado ao limiar.
///
/// A forma cobre os dois workloads. No Adult, `aᵢ` são atributos e `bᵢ` pesos do
/// modelo logístico. No ECEF, `aᵢ = bᵢ = leste_i - oeste_i`, de modo que a soma
/// é a distância ao quadrado. Uma forma só evita duas implementações por família
/// — seriam oito — e garante que as duas medições usem a mesma ordem de
/// acumulação, que é o que determina o erro.
#[derive(Clone, Debug)]
struct DecisionCase {
    bias: BigRational,
    terms: Vec<(BigRational, BigRational)>,
}

impl DecisionCase {
    fn exact_score(&self) -> BigRational {
        self.terms
            .iter()
            .fold(self.bias.clone(), |acc, (left, right)| acc + left * right)
    }
}

#[derive(Deserialize)]
struct AdultArtifact {
    bias: f64,
    weights: Vec<f64>,
    cases: Vec<AdultCase>,
}

#[derive(Deserialize)]
struct AdultCase {
    features: Vec<f64>,
}

fn float_ratio(value: f64) -> BigRational {
    BigRational::from_float(value).expect("valor finito")
}

fn adult_cases(path: &Path) -> Result<Vec<DecisionCase>> {
    let artifact: AdultArtifact = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("leitura de {}", path.display()))?,
    )
    .context("artefato Adult inválido")?;
    let bias = float_ratio(artifact.bias);
    let weights: Vec<BigRational> = artifact.weights.iter().copied().map(float_ratio).collect();
    Ok(artifact
        .cases
        .iter()
        .map(|case| DecisionCase {
            bias: bias.clone(),
            terms: case
                .features
                .iter()
                .copied()
                .map(float_ratio)
                .zip(weights.iter().cloned())
                .collect(),
        })
        .collect())
}

fn unit_vector(latitude: f64, longitude: f64) -> [f64; 3] {
    let cos_latitude = latitude.cos();
    [
        cos_latitude * longitude.cos(),
        cos_latitude * longitude.sin(),
        latitude.sin(),
    ]
}

fn destination(latitude: f64, longitude: f64, bearing: f64, distance: f64) -> (f64, f64) {
    let sin_latitude = latitude.sin();
    let cos_latitude = latitude.cos();
    let sin_distance = distance.sin();
    let cos_distance = distance.cos();
    let target_latitude =
        (sin_latitude * cos_distance + cos_latitude * sin_distance * bearing.cos()).asin();
    let target_longitude = longitude
        + (bearing.sin() * sin_distance * cos_latitude)
            .atan2(cos_distance - sin_latitude * target_latitude.sin());
    (target_latitude, target_longitude)
}

/// Casos ECEF com o mesmo gerador determinístico da rodada 0008.
///
/// A constante do LCG e a concentração de metade dos casos a `1e-4` do limiar
/// são reproduzidas literalmente: mudá-las tornaria os números desta rodada
/// incomparáveis com o achado das 494 decisões erradas já publicado.
fn ecef_cases(count: usize, threshold_angle: f64) -> Vec<DecisionCase> {
    let mut state = 0x0cbd_2026_0705_u64;
    let mut next_unit = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 11) as f64 / ((1_u64 << 53) as f64)
    };
    (0..count)
        .map(|index| {
            let latitude = (next_unit() - 0.5) * 2.4;
            let longitude = (next_unit() - 0.5) * std::f64::consts::TAU;
            let bearing = next_unit() * std::f64::consts::TAU;
            let factor = if index % 2 == 0 {
                1.0 + (next_unit() - 0.5) * 2e-4
            } else {
                0.5 + next_unit()
            };
            let (target_latitude, target_longitude) =
                destination(latitude, longitude, bearing, threshold_angle * factor);
            let left = unit_vector(latitude, longitude);
            let right = unit_vector(target_latitude, target_longitude);
            // A distância ao quadrado vira `Σ dᵢ·dᵢ`, cabendo na mesma forma.
            let terms = left
                .iter()
                .zip(&right)
                .map(|(a, b)| {
                    let difference = float_ratio(*a) - float_ratio(*b);
                    (difference.clone(), difference)
                })
                .collect();
            DecisionCase {
                bias: BigRational::zero(),
                terms,
            }
        })
        .collect()
}

/// Referência exata: os dois workloads são somas finitas de produtos de
/// racionais, sem iteração, então o valor exato é computável e não há
/// oportunidade inconclusiva.
fn decision_reference(cases: &[DecisionCase], thresholds: &[BigRational]) -> Reference {
    let scores: Vec<BigRational> = cases.iter().map(DecisionCase::exact_score).collect();
    let intervals = scores
        .iter()
        .map(|score| (score.clone(), score.clone()))
        .collect();
    let outcomes = thresholds
        .iter()
        .map(|threshold| {
            scores
                .iter()
                .map(|score| Some(score >= threshold))
                .collect()
        })
        .collect();
    Reference {
        intervals,
        outcomes,
    }
}

/// Quantiza na grade `2^-bits` com o modo pedido, sem sinal separado: o
/// arredondamento age sobre o valor com sinal, como no circuito.
fn quantize_signed(value: &BigRational, bits: usize, rounding: CircuitRounding) -> BigRational {
    let scale = BigInt::one() << bits;
    let scaled = value * BigRational::from_integer(scale.clone());
    let floor = scaled.floor();
    let remainder = &scaled - &floor;
    let half = BigRational::new(1.into(), 2.into());
    let rounded = match rounding {
        CircuitRounding::Truncate => floor,
        CircuitRounding::NearestTiesToEven => {
            if remainder > half {
                floor + BigRational::one()
            } else if remainder < half {
                floor
            } else if floor.to_integer().bit(0) {
                floor + BigRational::one()
            } else {
                floor
            }
        }
    };
    rounded / BigRational::from_integer(scale)
}

fn fixed_point_decision_states(
    cases: &[DecisionCase],
    bits: usize,
    rounding: CircuitRounding,
) -> Vec<StateView> {
    cases
        .iter()
        .map(|case| {
            let mut accumulator = quantize_signed(&case.bias, bits, rounding);
            for (left, right) in &case.terms {
                let product =
                    quantize_signed(left, bits, rounding) * quantize_signed(right, bits, rounding);
                accumulator = quantize_signed(&(accumulator + product), bits, rounding);
            }
            StateView::exact(accumulator)
        })
        .collect()
}

fn cbd_decision_states(
    cases: &[DecisionCase],
    bits: usize,
    thresholds: &[BigRational],
) -> Result<Vec<StateView>> {
    // Idêntico ao `signed_profile` de `workload_eval.rs`, inclusive na cadeia de
    // `next_fractional_bits`: os números desta rodada precisam reproduzir os de
    // 0008 para que o achado das 494 decisões continue citável.
    let profile = NumericProfile::new(
        format!("cbd-signed-workload-{bits}"),
        1,
        bits,
        bits + 7,
        BigRational::from_integer((-64).into()),
        BigRational::from_integer(64.into()),
        thresholds.to_vec(),
        match bits {
            16 => Some(32),
            32 => Some(48),
            48 => Some(64),
            _ => None,
        },
    )
    .context("perfil CBD assinado")?;
    cases
        .iter()
        .map(|case| {
            let bias = CertifiedDyadic::from_exact(profile.clone(), &case.bias)?;
            let terms = case
                .terms
                .iter()
                .map(|(left, right)| {
                    Ok((
                        CertifiedDyadic::from_exact(profile.clone(), left)?,
                        CertifiedDyadic::from_exact(profile.clone(), right)?,
                    ))
                })
                .collect::<Result<Vec<_>, numeric_core::CbdError>>()?;
            let score = CertifiedDyadic::linear_score(&bias, &terms)?;
            Ok(StateView {
                lower: score.lower(),
                upper: score.upper(),
                wide: false,
            })
        })
        .collect::<Result<Vec<_>, numeric_core::CbdError>>()
        .context("pontuação CBD")
}

fn cbd_score(
    case: &DecisionCase,
    profile: &NumericProfile,
) -> Result<CertifiedDyadic, numeric_core::CbdError> {
    let bias = CertifiedDyadic::from_exact(profile.clone(), &case.bias)?;
    let terms = case
        .terms
        .iter()
        .map(|(left, right)| {
            Ok((
                CertifiedDyadic::from_exact(profile.clone(), left)?,
                CertifiedDyadic::from_exact(profile.clone(), right)?,
            ))
        })
        .collect::<Result<Vec<_>, numeric_core::CbdError>>()?;
    CertifiedDyadic::linear_score(&bias, &terms)
}

fn cdad_decision_states(
    cases: &[DecisionCase],
    threshold: &BigRational,
    policy: PrecisionPolicy,
) -> Result<(Vec<StateView>, AdaptiveMetrics)> {
    let profile = DecisionAdaptiveProfile::signed_score(policy, vec![threshold.clone()])
        .context("perfil CDAD assinado")?;
    let mut metrics = AdaptiveMetrics::default();
    let mut states = Vec::with_capacity(cases.len());
    for case in cases {
        let adaptive = profile
            .evaluate(threshold, |numeric| cbd_score(case, numeric))
            .context("pontuação adaptativa CDAD")?;
        metrics.observe(&adaptive, profile.precision_ladder());
        states.push(StateView {
            lower: adaptive.value.lower(),
            upper: adaptive.value.upper(),
            wide: false,
        });
    }
    Ok((states, metrics))
}

fn cfd_signed_profile(
    precision_bits: usize,
    lanes: &[i16],
    radius_bits: usize,
    thresholds: &[BigRational],
) -> Result<CfdProfile> {
    CfdProfile::new(
        format!(
            "cfd-signed-p{precision_bits}-e{}_{}-r{radius_bits}",
            lanes.first().copied().unwrap_or_default(),
            lanes.last().copied().unwrap_or_default()
        ),
        1,
        precision_bits,
        lanes.to_vec(),
        radius_bits,
        BigRational::from_integer((-64).into()),
        BigRational::from_integer(64.into()),
        vec![CfdOperation::Add, CfdOperation::Multiply, CfdOperation::Fma],
        thresholds.to_vec(),
        None,
    )
    .context("perfil CFD assinado inválido")
}

fn cfd_decision_states(cases: &[DecisionCase], profile: &CfdProfile) -> Result<Vec<StateView>> {
    cases
        .iter()
        .map(|case| {
            let mut accumulator = CertifiedFloat::from_exact(profile.clone(), &case.bias)
                .context("viés fora do perfil CFD assinado")?;
            for (left, right) in &case.terms {
                let value = CertifiedFloat::from_exact(profile.clone(), left)
                    .context("fator fora do perfil CFD assinado")?;
                let weight = CertifiedFloat::from_exact(profile.clone(), right)
                    .context("peso fora do perfil CFD assinado")?;
                accumulator = value
                    .fma(&weight, &accumulator)
                    .context("fma CFD assinado")?;
            }
            Ok(StateView {
                lower: accumulator.lower(),
                upper: accumulator.upper(),
                wide: accumulator.is_wide(),
            })
        })
        .collect()
}

fn fp32_decision_states(cases: &[DecisionCase]) -> Result<Vec<StateView>> {
    cases
        .iter()
        .map(|case| {
            let mut accumulator = case.bias.to_f32().context("viés em binary32")?;
            for (left, right) in &case.terms {
                let value = left.to_f32().context("fator em binary32")?;
                let weight = right.to_f32().context("peso em binary32")?;
                accumulator += value * weight;
            }
            if !accumulator.is_finite() {
                bail!("acumulador FP32 não finito");
            }
            Ok(StateView::exact(
                BigRational::from_float(accumulator as f64).context("valor FP32 finito")?,
            ))
        })
        .collect()
}

/// Matriz dos workloads de decisão.
///
/// A CFD usa corredor `-32:8`: os dois workloads acumulam produtos que saem de
/// `[0,1]` e o corredor do mapa logístico não os cobre.
fn decision_variants() -> Vec<Variant> {
    let mut variants = Vec::new();
    for bits in [16, 32, 48, 64] {
        for rounding in [RoundingId::Truncate, RoundingId::NearestTiesToEven] {
            variants.push(Variant::plain(RepresentationId::Standard, bits, rounding));
        }
        variants.push(Variant::plain(
            RepresentationId::CbdLocalTaylor,
            bits,
            RoundingId::NearestTiesToEven,
        ));
    }
    for precision_bits in [16, 24, 32] {
        variants.push(Variant {
            representation: RepresentationId::CfdSequential,
            precision_bits,
            rounding: RoundingId::HonestRneCertifiedEnclosure,
            exponent_lanes: (-32..=8).collect(),
            radius_bits: 16,
        });
    }
    variants.push(Variant::plain(
        RepresentationId::Fp32Exact,
        24,
        RoundingId::NearestTiesToEven,
    ));
    for representation in [
        RepresentationId::CdadFixed,
        RepresentationId::CdadSequential,
        RepresentationId::CdadMarginDirected,
    ] {
        variants.push(Variant::plain(
            representation,
            16,
            RoundingId::NearestTiesToEven,
        ));
    }
    variants
}

fn decision_states(
    variant: &Variant,
    cases: &[DecisionCase],
    thresholds: &[BigRational],
    threshold: &BigRational,
) -> Result<(Vec<StateView>, Option<AdaptiveMetrics>)> {
    match variant.representation {
        RepresentationId::Standard => Ok((
            fixed_point_decision_states(
                cases,
                variant.precision_bits,
                circuit_rounding(variant.rounding)?,
            ),
            None,
        )),
        RepresentationId::CbdLocalTaylor => Ok((
            cbd_decision_states(cases, variant.precision_bits, thresholds)?,
            None,
        )),
        RepresentationId::CdadFixed
        | RepresentationId::CdadSequential
        | RepresentationId::CdadMarginDirected => {
            let policy = variant
                .representation
                .precision_policy()
                .expect("representação CDAD possui política");
            let (states, metrics) = cdad_decision_states(cases, threshold, policy)?;
            Ok((states, Some(metrics)))
        }
        RepresentationId::CfdSequential => {
            let profile = cfd_signed_profile(
                variant.precision_bits,
                &variant.exponent_lanes,
                variant.radius_bits,
                thresholds,
            )?;
            Ok((cfd_decision_states(cases, &profile)?, None))
        }
        RepresentationId::Fp32Exact => Ok((fp32_decision_states(cases)?, None)),
        other => bail!("{} não avalia workloads de decisão", other.tag()),
    }
}

fn sweep_decisions(
    variants: &[Variant],
    adult_model: &Path,
    ecef_count: usize,
    records: &mut Vec<CoverageRecord>,
) -> Result<()> {
    // Raio de 100 km sobre uma Terra de 6.371 km, exatamente como em 0008. Os
    // valores precisam ser estes e não outros: mudar o raio muda quão perto do
    // limiar os casos caem, e é disso que a cobertura depende inteiramente. Um
    // limiar mais apertado faria o CBD-16 abster-se mais e produziria um
    // contraste mais favorável ao argumento — motivo de sobra para não escolher.
    let radius_km = 100.0_f64;
    let earth_radius_km = 6_371.0_f64;
    let threshold_angle = radius_km / earth_radius_km;
    // Corda ao quadrado: `(2 sin(θ/2))² = 2 − 2 cos θ`.
    let ecef_threshold = float_ratio(2.0 - 2.0 * threshold_angle.cos());

    let workloads: Vec<(
        WorkloadId,
        &'static str,
        Vec<DecisionCase>,
        Vec<BigRational>,
    )> = vec![
        (
            WorkloadId::AdultLogistic,
            "decisao_unica",
            adult_cases(adult_model)?,
            vec![BigRational::zero()],
        ),
        (
            WorkloadId::EcefProximity,
            "proximidade",
            ecef_cases(ecef_count, threshold_angle),
            vec![ecef_threshold],
        ),
    ];

    for (workload, regime, cases, thresholds) in workloads {
        let reference = decision_reference(&cases, &thresholds);
        let spec = WorkloadSpec::signed_scoring(
            workload,
            [(
                "cases".to_string(),
                BigRational::from_integer(cases.len().into()),
            )],
        );
        for variant in variants {
            for (index, threshold) in thresholds.iter().enumerate() {
                let (states, adaptive) = decision_states(variant, &cases, &thresholds, threshold)
                    .with_context(|| {
                    format!(
                        "pontuação de {} p{} em {:?}",
                        variant.representation.tag(),
                        variant.precision_bits,
                        workload
                    )
                })?;
                let decisions = decide_all(&states, std::slice::from_ref(threshold));
                let counts = score(
                    &states,
                    &decisions[0],
                    &reference,
                    index,
                    variant.representation.is_certified(),
                );
                records.push(record(
                    variant,
                    &spec,
                    regime,
                    threshold,
                    true,
                    states.len(),
                    counts,
                    adaptive.as_ref(),
                )?);
            }
        }
        eprintln!("{workload:?}: {} registros acumulados", records.len());
    }
    Ok(())
}

fn regime_of(r: &BigRational) -> &'static str {
    if *r == decimal_ratio(7, 2) {
        "estavel"
    } else {
        "caotico"
    }
}

fn selected(variants: Vec<Variant>, only: Option<&str>) -> Vec<Variant> {
    variants
        .into_iter()
        .filter(|variant| only.map_or(true, |tag| variant.representation.tag() == tag))
        .collect()
}

/// Varredura do mapa logístico: quatro valores de `r`, do periódico ao caótico.
fn sweep_logistic(
    variants: &[Variant],
    steps: usize,
    thresholds: &[BigRational],
    principal: &BigRational,
    records: &mut Vec<CoverageRecord>,
) -> Result<()> {
    let x0 = decimal_ratio(1, 10);
    for r in [
        decimal_ratio(7, 2),
        decimal_ratio(37, 10),
        decimal_ratio(39, 10),
        decimal_ratio(4, 1),
    ] {
        let reference = logistic_reference(&x0, &r, steps, thresholds)
            .with_context(|| format!("referência MPFR para r={r}"))?;
        let spec = WorkloadSpec::logistic_map(r.clone(), x0.clone());
        let regime = regime_of(&r);
        for variant in variants {
            if let Some(policy) = variant.representation.precision_policy() {
                let adaptive = cdad_logistic_trajectories(&x0, &r, steps, thresholds, policy)
                    .with_context(|| {
                        format!(
                            "trajetória adaptativa de {} r={r}",
                            variant.representation.tag()
                        )
                    })?;
                for (index, (trajectory, metrics)) in adaptive.iter().enumerate() {
                    let threshold = &thresholds[index];
                    let counts = score(
                        &trajectory.states,
                        &trajectory.decisions[0],
                        &reference,
                        index,
                        true,
                    );
                    records.push(record(
                        variant,
                        &spec,
                        regime,
                        threshold,
                        threshold == principal,
                        trajectory.states.len(),
                        counts,
                        Some(metrics),
                    )?);
                }
                continue;
            }
            let trajectory =
                trajectory_for(variant, &x0, &r, steps, thresholds).with_context(|| {
                    format!(
                        "trajetória de {} p{} r={r}",
                        variant.representation.tag(),
                        variant.precision_bits
                    )
                })?;
            let states = trajectory.states.len();
            for (index, threshold) in thresholds.iter().enumerate() {
                let counts = score(
                    &trajectory.states,
                    &trajectory.decisions[index],
                    &reference,
                    index,
                    variant.representation.is_certified(),
                );
                records.push(record(
                    variant,
                    &spec,
                    regime,
                    threshold,
                    threshold == principal,
                    states,
                    counts,
                    None,
                )?);
            }
        }
        eprintln!("logístico r={r}: {} registros acumulados", records.len());
    }
    Ok(())
}

/// Varredura afim: dois `alpha`, um diádico e um não diádico.
///
/// `alpha = 1/2` é exatamente representável na grade binária, e a etapa de ponto
/// fixo não arredonda nada — o formato não certificado acerta tudo por
/// construção. `alpha = 7/10` não é diádico e obriga arredondamento a cada
/// passo. Sem os dois casos, o regime contrativo pareceria trivialmente
/// favorável ao ponto fixo por uma propriedade do parâmetro, não do regime.
fn sweep_affine(
    variants: &[Variant],
    steps: usize,
    thresholds: &[BigRational],
    principal: &BigRational,
    records: &mut Vec<CoverageRecord>,
) -> Result<()> {
    let x0 = decimal_ratio(1, 10);
    let cases = [
        (decimal_ratio(1, 2), decimal_ratio(3, 4), "diadico"),
        (decimal_ratio(7, 10), decimal_ratio(1, 2), "nao_diadico"),
    ];
    for (alpha, innovation, regime) in cases {
        let parameters = AffineParameters::new(
            alpha.numer().to_u64().context("numerador de alpha")?,
            alpha.denom().to_u64().context("denominador de alpha")?,
            innovation
                .numer()
                .to_u64()
                .context("numerador da inovação")?,
            innovation
                .denom()
                .to_u64()
                .context("denominador da inovação")?,
        );
        let reference = affine_reference(&x0, &alpha, &innovation, steps, thresholds)
            .with_context(|| format!("referência MPFR para alpha={alpha}"))?;
        let spec = WorkloadSpec::stable_affine(alpha.clone(), innovation.clone(), x0.clone());
        for variant in variants {
            if let Some(policy) = variant.representation.precision_policy() {
                let adaptive =
                    cdad_affine_trajectories(&x0, &alpha, &innovation, steps, thresholds, policy)?;
                for (index, (trajectory, metrics)) in adaptive.iter().enumerate() {
                    let threshold = &thresholds[index];
                    let counts = score(
                        &trajectory.states,
                        &trajectory.decisions[0],
                        &reference,
                        index,
                        true,
                    );
                    records.push(record(
                        variant,
                        &spec,
                        regime,
                        threshold,
                        threshold == principal,
                        trajectory.states.len(),
                        counts,
                        Some(metrics),
                    )?);
                }
                continue;
            }
            let trajectory = affine_trajectory_for(
                variant,
                &x0,
                &alpha,
                &innovation,
                parameters,
                steps,
                thresholds,
            )
            .with_context(|| {
                format!(
                    "trajetória afim de {} p{} alpha={alpha}",
                    variant.representation.tag(),
                    variant.precision_bits
                )
            })?;
            let states = trajectory.states.len();
            for (index, threshold) in thresholds.iter().enumerate() {
                let counts = score(
                    &trajectory.states,
                    &trajectory.decisions[index],
                    &reference,
                    index,
                    variant.representation.is_certified(),
                );
                records.push(record(
                    variant,
                    &spec,
                    regime,
                    threshold,
                    threshold == principal,
                    states,
                    counts,
                    None,
                )?);
            }
        }
        eprintln!("afim alpha={alpha}: {} registros acumulados", records.len());
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let logistic_thresholds = [
        decimal_ratio(1, 4),
        decimal_ratio(1, 2),
        decimal_ratio(3, 4),
    ];
    let logistic_principal = decimal_ratio(1, 2);
    // O domínio afim é `[0,2)` e o atrator fica acima de 1; limiares abaixo de
    // 1 seriam decididos no primeiro passo e não distinguiriam nada.
    let affine_thresholds = [
        decimal_ratio(1, 1),
        decimal_ratio(5, 4),
        decimal_ratio(3, 2),
    ];
    let affine_principal = decimal_ratio(1, 1);

    let mut records = Vec::new();
    let logistic = selected(logistic_variants(), args.only.as_deref());
    let affine = selected(affine_variants(), args.only.as_deref());
    let decisions = selected(decision_variants(), args.only.as_deref());
    if logistic.is_empty() && affine.is_empty() && decisions.is_empty() {
        bail!("nenhuma representação corresponde a --only");
    }
    if !logistic.is_empty() {
        sweep_logistic(
            &logistic,
            args.steps,
            &logistic_thresholds,
            &logistic_principal,
            &mut records,
        )?;
    }
    if !affine.is_empty() {
        sweep_affine(
            &affine,
            args.steps,
            &affine_thresholds,
            &affine_principal,
            &mut records,
        )?;
    }
    if !decisions.is_empty() {
        sweep_decisions(&decisions, &args.adult_model, args.ecef_cases, &mut records)?;
    }

    let evaluation = Evaluation {
        schema_version: SCHEMA_VERSION,
        experiment: "cobertura homogênea da fronteira de Pareto",
        reference: "MPFR com bound conservador; oportunidades inconclusivas excluídas",
        steps: args.steps,
        records,
    };
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, serde_json::to_vec_pretty(&evaluation)?)?;
    eprintln!(
        "{} registros gravados em {}",
        evaluation.records.len(),
        args.output.display()
    );
    Ok(())
}

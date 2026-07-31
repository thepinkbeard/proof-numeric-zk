use anyhow::{Context, Result};
use clap::Parser;
use nova_circuit::{CfdCircuitValue, CfdLogisticStepWitness, CfdTopology};
use num_bigint::BigUint;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};
use numeric_core::{CertifiedFloat, CfdOperation, CfdProfile, Decision3};
use numeric_reference::{decimal_ratio, logistic_trajectory_high_precision, HighPrecisionConfig};
use serde::Serialize;
use std::{fmt::Write, fs, path::PathBuf};

#[derive(Debug, Parser)]
#[command(about = "Avalia cobertura e soundness da Certified Floating Dyadic")]
struct Args {
    #[arg(long, default_value_t = 1000)]
    steps: usize,
    #[arg(long, default_value = "results/cfd_workloads.json")]
    output: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
struct WorkloadMetrics {
    workload: String,
    variant: String,
    precision_bits: usize,
    exponent_lanes: Vec<i16>,
    profile_hash_hex: String,
    states: usize,
    first_wide: Option<usize>,
    definite_025: Option<usize>,
    definite_050: Option<usize>,
    definite_075: Option<usize>,
    definite_100: Option<usize>,
    indeterminate: usize,
    unsound_definitive_decisions: usize,
    enclosure_violations: usize,
    mean_interval_width: f64,
    maximum_interval_width: f64,
}

#[derive(Debug, Serialize)]
struct Evaluation {
    experiment: &'static str,
    reference: &'static str,
    steps: usize,
    metrics: Vec<WorkloadMetrics>,
}

fn logistic_profile(p: usize, minimum_exponent: i16) -> CfdProfile {
    CfdProfile::new(
        format!("cfd-logistic-p{p}-e{minimum_exponent}"),
        1,
        p,
        (minimum_exponent..=0).collect(),
        16,
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
        vec![
            BigRational::new(1.into(), 4.into()),
            BigRational::new(1.into(), 2.into()),
            BigRational::new(3.into(), 4.into()),
        ],
        None,
    )
    .expect("perfil logístico CFD")
}

fn hash_hex(hash: [u8; 32]) -> String {
    hash.iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            write!(out, "{byte:02x}").expect("escrita em String não falha");
            out
        })
}

fn from_circuit_value(profile: &CfdProfile, value: &CfdCircuitValue) -> Result<CertifiedFloat> {
    CertifiedFloat::from_parts(
        profile.clone(),
        false,
        value.exponent_lane as usize,
        BigUint::from(value.significand),
        BigUint::from(value.radius),
        value.zero,
        value.wide,
    )
    .map_err(Into::into)
}

fn logistic_metrics(
    steps: usize,
    r: &BigRational,
    p: usize,
    minimum_exponent: i16,
    fused: bool,
) -> Result<WorkloadMetrics> {
    let profile = logistic_profile(p, minimum_exponent);
    let x0 = BigRational::new(1.into(), 10.into());
    let reference = logistic_trajectory_high_precision(
        &x0,
        r,
        steps,
        &HighPrecisionConfig {
            precision_bits: (2 * steps + 128).max(256) as u32,
        },
    )?;
    let mut state = CertifiedFloat::from_exact(profile.clone(), &x0)?;
    let thresholds = [0.25_f64, 0.5, 0.75];
    let mut definite = [0_usize; 3];
    let mut unsound = 0;
    let mut enclosure_violations = 0;
    let mut first_wide = None;
    let mut width_sum = 0.0;
    let mut maximum_width = 0.0_f64;
    let mut indeterminate = 0;
    for (iteration, reference_point) in reference.iter().enumerate() {
        let lower = state.lower().to_f64().unwrap_or(f64::NEG_INFINITY);
        let upper = state.upper().to_f64().unwrap_or(f64::INFINITY);
        let reference_value = reference_point.value.to_f64();
        let reference_error = reference_point.estimated_absolute_error.to_f64();
        if lower > reference_value - reference_error || upper < reference_value + reference_error {
            enclosure_violations += 1;
        }
        let width = upper - lower;
        width_sum += width;
        maximum_width = maximum_width.max(width);
        if state.is_wide() {
            first_wide.get_or_insert(iteration);
        }
        for (index, threshold) in thresholds.iter().copied().enumerate() {
            let threshold_rational = BigRational::from_float(threshold).expect("dyadic threshold");
            match state.ge_threshold(&threshold_rational) {
                Decision3::True => {
                    definite[index] += 1;
                    unsound += usize::from(reference_value + reference_error < threshold);
                }
                Decision3::False => {
                    definite[index] += 1;
                    unsound += usize::from(reference_value - reference_error >= threshold);
                }
                Decision3::Indeterminate => indeterminate += 1,
            }
        }
        if iteration < steps {
            let topology = if fused {
                CfdTopology::Fused
            } else {
                CfdTopology::Sequential
            };
            let witness = CfdLogisticStepWitness::compute(
                &profile,
                &state,
                r.numer().to_u64().context("numerador logístico")?,
                r.denom().to_u64().context("denominador logístico")?,
                topology,
            )
            .with_context(|| format!("testemunha CFD na iteração {iteration}"))?;
            state = from_circuit_value(&profile, &witness.output)
                .with_context(|| format!("estado circuital CFD na iteração {iteration}"))?;
        }
    }
    Ok(WorkloadMetrics {
        workload: format!("logistic-r{}", r.to_f64().unwrap()),
        variant: if fused { "fused" } else { "sequential" }.into(),
        precision_bits: p,
        exponent_lanes: profile.exponent_lanes().to_vec(),
        profile_hash_hex: hash_hex(profile.profile_hash()),
        states: steps + 1,
        first_wide,
        definite_025: Some(definite[0]),
        definite_050: Some(definite[1]),
        definite_075: Some(definite[2]),
        definite_100: None,
        indeterminate,
        unsound_definitive_decisions: unsound,
        enclosure_violations,
        mean_interval_width: width_sum / (steps + 1) as f64,
        maximum_interval_width: maximum_width,
    })
}

fn stable_metrics(steps: usize, p: usize) -> Result<WorkloadMetrics> {
    let profile = CfdProfile::stable_score(p)?;
    let alpha = BigRational::new(1.into(), 2.into());
    let innovation_exact = BigRational::new(3.into(), 4.into());
    let innovation = CertifiedFloat::from_exact(profile.clone(), &innovation_exact)?;
    let mut exact = BigRational::new(1.into(), 10.into());
    let mut state = CertifiedFloat::from_exact(profile.clone(), &exact)?;
    let threshold = BigRational::one();
    let mut definite = 0;
    let mut indeterminate = 0;
    let mut unsound = 0;
    let mut enclosure_violations = 0;
    let mut first_wide = None;
    let mut width_sum = 0.0;
    let mut maximum_width = 0.0_f64;
    for iteration in 0..=steps {
        if !state.contains(&exact) {
            enclosure_violations += 1;
        }
        let width = (state.upper() - state.lower())
            .to_f64()
            .unwrap_or(f64::INFINITY);
        width_sum += width;
        maximum_width = maximum_width.max(width);
        if state.is_wide() {
            first_wide.get_or_insert(iteration);
        }
        match state.ge_threshold(&threshold) {
            Decision3::True => {
                definite += 1;
                unsound += usize::from(exact < threshold);
            }
            Decision3::False => {
                definite += 1;
                unsound += usize::from(exact >= threshold);
            }
            Decision3::Indeterminate => indeterminate += 1,
        }
        if iteration < steps {
            exact = &alpha * exact + &innovation_exact;
            state = state.stable_affine_step(&alpha, &innovation)?;
        }
    }
    Ok(WorkloadMetrics {
        workload: "stable-affine-alpha-0.5-innovation-0.75".into(),
        variant: "sequential".into(),
        precision_bits: p,
        exponent_lanes: profile.exponent_lanes().to_vec(),
        profile_hash_hex: hash_hex(profile.profile_hash()),
        states: steps + 1,
        first_wide,
        definite_025: None,
        definite_050: None,
        definite_075: None,
        definite_100: Some(definite),
        indeterminate,
        unsound_definitive_decisions: unsound,
        enclosure_violations,
        mean_interval_width: width_sum / (steps + 1) as f64,
        maximum_interval_width: maximum_width,
    })
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut metrics = Vec::new();
    for r in [
        decimal_ratio(7, 2),
        decimal_ratio(37, 10),
        decimal_ratio(39, 10),
        decimal_ratio(4, 1),
    ] {
        for (p, minimum_exponent) in [(16, -8), (24, -4), (24, -8), (24, -16), (32, -8)] {
            metrics.push(
                logistic_metrics(args.steps, &r, p, minimum_exponent, false).with_context(
                    || format!("logistic r={r} p={p} e={minimum_exponent} sequential"),
                )?,
            );
            metrics.push(
                logistic_metrics(args.steps, &r, p, minimum_exponent, true)
                    .with_context(|| format!("logistic r={r} p={p} e={minimum_exponent} fused"))?,
            );
        }
    }
    for p in [16, 24, 32] {
        metrics.push(stable_metrics(args.steps, p).with_context(|| format!("stable p={p}"))?);
    }
    let evaluation = Evaluation {
        experiment: "Certified Floating Dyadic cost-coverage workloads",
        reference: "rational exact for stable affine; MPFR with conservative bound for logistic",
        steps: args.steps,
        metrics,
    };
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, serde_json::to_vec_pretty(&evaluation)?)?;
    println!("{}", serde_json::to_string_pretty(&evaluation)?);
    Ok(())
}

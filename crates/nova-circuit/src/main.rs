use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use nova_circuit::{
    circuit_decision, common_threshold, initial_fixed_point_state, power_of_two_biased_exponent,
    AffineParameters, AffineStepCircuit, AffineStepWitness, CarryLogisticStepCircuit,
    CbdAffineStepCircuit, CbdAffineStepWitness, CbdLogisticStepCircuit, CbdRadiusBound,
    CbdSequentialLogisticStepCircuit, CbdSequentialStepWitness, CbdStepWitness,
    CdadFixedLogisticStepCircuit, CfdLogisticStepCircuit, CfdLogisticStepWitness,
    CfdStableStepCircuit, CfdStableStepWitness, CfdTopology, CircuitFp32, CircuitRounding,
    CompensatedLogisticStepCircuit, CompensatedStepWitness, Fp32MulCircuit, Fp32MulWitness,
    Fp32SubOneCircuit, Fp32SubOneWitness, LogisticStepCircuit, ProductCarryWitness,
    RestrictedFp32StableStepCircuit, RestrictedFp32StableStepWitness, RestrictedFp32StepCircuit,
    RestrictedFp32StepWitness, StepWitness,
};
use nova_snark::{
    frontend::{num::AllocatedNum, test_cs::TestConstraintSystem, ConstraintSystem},
    nova::{CompressedSNARK, PublicParams, RecursiveSNARK},
    provider::{ipa_pc::EvaluationEngine, PallasEngine, VestaEngine},
    spartan::snark::RelaxedR1CSSNARK,
    traits::{circuit::StepCircuit, snark::RelaxedR1CSSNARKTrait, Engine},
};
use num_bigint::{BigInt, BigUint};
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use numeric_core::{
    CertifiedFloat, CfdOperation, CfdProfile, Decision3, ExperimentProfile, FrontierPoint,
    ProfileFamily, RepresentationId, RoundingId, WorkloadId, WorkloadSpec,
};
use serde::Serialize;
use std::{fs, path::PathBuf, time::Instant};

type E1 = PallasEngine;
type E2 = VestaEngine;
type EE1 = EvaluationEngine<E1>;
type EE2 = EvaluationEngine<E2>;
type S1 = RelaxedR1CSSNARK<E1, EE1>;
type S2 = RelaxedR1CSSNARK<E2, EE2>;
type Scalar = <E1 as Engine>::Scalar;

const LIMB_WIDTH_BITS: usize = 16;
const FP32_PRECISION_BITS: usize = 24;

#[derive(Debug, Parser)]
#[command(about = "Executa IVC Nova para os workloads do protocolo de Pareto")]
struct Args {
    #[arg(long, default_value_t = 10)]
    steps: usize,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=4))]
    k: u8,
    #[arg(long)]
    compress: bool,
    #[arg(long, default_value = "truncate")]
    rounding: RoundingArg,
    #[arg(long, default_value = "standard")]
    representation: RepresentationArg,
    #[arg(long, default_value = "logistic-map")]
    workload: WorkloadArg,
    /// Precisão em bits. CFD: significando `p`. Ponto fixo e CBD: bits
    /// fracionários, que devem coincidir com `16*k`. FP32: sempre 24.
    #[arg(long)]
    precision: Option<usize>,
    /// Corredor de expoentes CFD no formato `min:max`, por exemplo `-8:0`.
    #[arg(long)]
    exponent_lanes: Option<String>,
    /// Largura do campo de raio CFD.
    #[arg(long)]
    radius_bits: Option<usize>,
    /// Limiar público da decisão, racional. Aceita `1/2` ou `0.5`.
    #[arg(long)]
    threshold: Option<String>,
    /// Parâmetro do mapa logístico.
    #[arg(long, default_value = "37/10")]
    r: String,
    /// Estado inicial da trajetória.
    #[arg(long, default_value = "1/10")]
    x0: String,
    /// Fator de contração da recorrência afim estável.
    #[arg(long, default_value = "1/2")]
    alpha: String,
    /// Termo de inovação da recorrência afim estável.
    #[arg(long, default_value = "3/4")]
    innovation: String,
    #[arg(long, default_value = "results/nova_metrics.json")]
    output: PathBuf,
    /// Export an independently verifiable compressed proof (new directory only).
    #[arg(long, conflicts_with = "verify_proof")]
    export_proof: Option<PathBuf>,
    /// Verify against the independently supplied CLI statement; never trust a supplied verification key.
    #[arg(long, conflicts_with = "export_proof")]
    verify_proof: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum RoundingArg {
    Truncate,
    NearestEven,
}

impl From<RoundingArg> for CircuitRounding {
    fn from(value: RoundingArg) -> Self {
        match value {
            RoundingArg::Truncate => Self::Truncate,
            RoundingArg::NearestEven => Self::NearestTiesToEven,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum WorkloadArg {
    LogisticMap,
    StableAffine,
}

impl WorkloadArg {
    fn label(self) -> &'static str {
        match self {
            Self::LogisticMap => "logistic-map",
            Self::StableAffine => "stable-affine",
        }
    }
}

impl From<WorkloadArg> for WorkloadId {
    fn from(value: WorkloadArg) -> Self {
        match value {
            WorkloadArg::LogisticMap => Self::LogisticMap,
            WorkloadArg::StableAffine => Self::StableAffine,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum RepresentationArg {
    Standard,
    Carry,
    Compensated,
    Cbd,
    CbdGlobal,
    CbdSequential,
    Cdad,
    Cfd,
    CfdFused,
    CfdStable,
    Fp32,
    Fp32Stable,
}

impl RepresentationArg {
    /// Workload que o rótulo legado já implica, quando implica algum.
    fn implied_workload(self) -> Option<WorkloadArg> {
        match self {
            Self::CfdStable | Self::Fp32Stable => Some(WorkloadArg::StableAffine),
            // O circuito compensado calcula `x*(1-x)` no par `(V,E)`; não existe
            // versão afim dele, e o protocolo declara essa extensão fora de
            // escopo.
            Self::Compensated => Some(WorkloadArg::LogisticMap),
            Self::Standard
            | Self::Carry
            | Self::Cbd
            | Self::CbdGlobal
            | Self::CbdSequential
            | Self::Cdad
            | Self::Cfd
            | Self::CfdFused
            | Self::Fp32 => None,
        }
    }

    fn family(self) -> ProfileFamily {
        match self {
            Self::Standard | Self::Carry | Self::Compensated => ProfileFamily::FixedPoint,
            Self::Cbd | Self::CbdGlobal | Self::CbdSequential => ProfileFamily::Cbd,
            Self::Cdad => ProfileFamily::Cdad,
            Self::Cfd | Self::CfdFused | Self::CfdStable => ProfileFamily::Cfd,
            Self::Fp32 | Self::Fp32Stable => ProfileFamily::Fp32,
        }
    }
}

/// Configuração resolvida: nenhum parâmetro numérico permanece implícito.
struct Config {
    steps: usize,
    k: usize,
    compress: bool,
    rounding: CircuitRounding,
    representation: RepresentationArg,
    workload: WorkloadArg,
    precision_bits: usize,
    exponent_lanes: Vec<i16>,
    radius_bits: usize,
    threshold: BigRational,
    r: BigRational,
    x0: BigRational,
    alpha: BigRational,
    innovation: BigRational,
    output: PathBuf,
    export_proof: Option<PathBuf>,
    verify_proof: Option<PathBuf>,
}

fn parse_rational(text: &str) -> Result<BigRational> {
    let trimmed = text.trim();
    if let Some((numerator, denominator)) = trimmed.split_once('/') {
        let numerator: BigInt = numerator
            .trim()
            .parse()
            .with_context(|| format!("numerador inválido em {trimmed:?}"))?;
        let denominator: BigInt = denominator
            .trim()
            .parse()
            .with_context(|| format!("denominador inválido em {trimmed:?}"))?;
        if denominator.is_zero() {
            bail!("denominador nulo em {trimmed:?}");
        }
        return Ok(BigRational::new(numerator, denominator));
    }
    if let Some((integer, fraction)) = trimmed.split_once('.') {
        let negative = integer.starts_with('-');
        let integer_part: BigInt = if integer == "-" || integer.is_empty() {
            BigInt::zero()
        } else {
            integer
                .parse()
                .with_context(|| format!("parte inteira inválida em {trimmed:?}"))?
        };
        let digits = fraction.trim();
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            bail!("parte fracionária inválida em {trimmed:?}");
        }
        let fraction_part: BigInt = digits
            .parse()
            .with_context(|| format!("parte fracionária inválida em {trimmed:?}"))?;
        let denominator = BigInt::from(10u8).pow(digits.len() as u32);
        let magnitude = BigRational::new(
            integer_part.abs() * &denominator + fraction_part,
            denominator,
        );
        return Ok(if negative { -magnitude } else { magnitude });
    }
    let integer: BigInt = trimmed
        .parse()
        .with_context(|| format!("racional inválido em {trimmed:?}"))?;
    Ok(BigRational::from_integer(integer))
}

fn parse_lane_range(text: &str) -> Result<Vec<i16>> {
    let (minimum, maximum) = text
        .trim()
        .split_once(':')
        .with_context(|| format!("--exponent-lanes espera `min:max`, recebeu {text:?}"))?;
    let minimum: i16 = minimum
        .trim()
        .parse()
        .with_context(|| format!("expoente mínimo inválido em {text:?}"))?;
    let maximum: i16 = maximum
        .trim()
        .parse()
        .with_context(|| format!("expoente máximo inválido em {text:?}"))?;
    if minimum > maximum {
        bail!("--exponent-lanes exige min <= max, recebeu {text:?}");
    }
    Ok((minimum..=maximum).collect())
}

/// Rejeita corredores de expoente que o circuito CFD não consegue sintetizar.
///
/// As mesmas condições existem como `assert` em `cfd::validate_profile`; aqui
/// elas viram erro de CLI para que uma varredura da matriz experimental não
/// aborte com panic no meio da execução.
fn validate_cfd_corridor(
    lanes: &[i16],
    precision_bits: usize,
    workload: WorkloadArg,
) -> Result<()> {
    if lanes.is_empty() || lanes.len() > 32 {
        bail!(
            "--exponent-lanes deve descrever de 1 a 32 faixas, recebeu {}",
            lanes.len()
        );
    }
    let minimum = lanes[0];
    let maximum = *lanes.last().expect("corredor não vazio");
    let ceiling: i16 = match workload {
        WorkloadArg::LogisticMap => 0,
        WorkloadArg::StableAffine => 1,
    };
    if maximum > ceiling {
        bail!(
            "--exponent-lanes com expoente máximo {maximum} excede o domínio do workload {}, \
             cujo teto é {ceiling}",
            workload.label()
        );
    }
    let span = i32::from(maximum) - i32::from(minimum);
    if span > 63 {
        bail!("--exponent-lanes tem envergadura {span}, acima do limite 63 da escala comum");
    }
    let common_bits = precision_bits - 1 + (-i32::from(minimum)).max(0) as usize;
    if common_bits >= 63 {
        bail!(
            "--precision {precision_bits} com expoente mínimo {minimum} exige {common_bits} bits \
             na escala diádica comum, acima do limite 62; reduza a precisão ou encurte o corredor"
        );
    }
    Ok(())
}

fn rational_text(value: &BigRational) -> String {
    format!("{}/{}", value.numer(), value.denom())
}

fn small_ratio(value: &BigRational, name: &str) -> Result<(u64, u64)> {
    if value.is_negative() {
        bail!("--{name} deve ser não negativo");
    }
    let numerator = value
        .numer()
        .to_u64()
        .with_context(|| format!("--{name} não cabe em u64 no numerador"))?;
    let denominator = value
        .denom()
        .to_u64()
        .with_context(|| format!("--{name} não cabe em u64 no denominador"))?;
    Ok((numerator, denominator))
}

impl Config {
    fn resolve(args: &Args) -> Result<Self> {
        if args.steps == 0 {
            bail!("--steps deve ser positivo");
        }
        let representation = args.representation;
        let k = usize::from(args.k);

        let workload = match representation.implied_workload() {
            Some(implied) if implied != args.workload => bail!(
                "--representation {:?} implica --workload {:?}; remova a combinação explícita",
                representation,
                implied
            ),
            Some(implied) => implied,
            None => args.workload,
        };

        let family = representation.family();
        let precision_bits = match family {
            ProfileFamily::FixedPoint | ProfileFamily::Cbd | ProfileFamily::Cdad => {
                let derived = LIMB_WIDTH_BITS * k;
                if let Some(requested) = args.precision {
                    if requested != derived {
                        bail!(
                            "--precision {requested} é incompatível com --k {k}: \
                             ponto fixo e CBD usam limbs de {LIMB_WIDTH_BITS} bits, \
                             logo a precisão é {derived}. Ajuste --k."
                        );
                    }
                }
                derived
            }
            ProfileFamily::Fp32 => {
                if let Some(requested) = args.precision {
                    if requested != FP32_PRECISION_BITS {
                        bail!(
                            "--precision {requested} não se aplica ao FP32 restrito, \
                             que é binary32 com p={FP32_PRECISION_BITS}"
                        );
                    }
                }
                FP32_PRECISION_BITS
            }
            ProfileFamily::Cfd => {
                let precision = args.precision.unwrap_or(FP32_PRECISION_BITS);
                if !(2..=63).contains(&precision) {
                    bail!("--precision CFD deve pertencer a 2..=63, recebeu {precision}");
                }
                precision
            }
        };

        let default_lanes = match workload {
            WorkloadArg::LogisticMap => (-8..=0).collect::<Vec<i16>>(),
            WorkloadArg::StableAffine => (-16..=1).collect::<Vec<i16>>(),
        };
        let exponent_lanes = match &args.exponent_lanes {
            Some(text) => parse_lane_range(text)?,
            None => default_lanes,
        };
        if family != ProfileFamily::Cfd && args.exponent_lanes.is_some() {
            bail!("--exponent-lanes só se aplica à família CFD");
        }
        let radius_bits = args.radius_bits.unwrap_or(16);
        if family != ProfileFamily::Cfd && args.radius_bits.is_some() {
            bail!("--radius-bits só se aplica à família CFD");
        }
        if !(1..=63).contains(&radius_bits) {
            bail!("--radius-bits deve pertencer a 1..=63, recebeu {radius_bits}");
        }
        if family == ProfileFamily::Cfd {
            validate_cfd_corridor(&exponent_lanes, precision_bits, workload)?;
        }

        let default_threshold = match workload {
            WorkloadArg::LogisticMap => BigRational::new(1.into(), 2.into()),
            WorkloadArg::StableAffine => BigRational::one(),
        };
        let threshold = match &args.threshold {
            Some(text) => parse_rational(text)?,
            None => default_threshold,
        };
        if !threshold.is_positive() {
            bail!("--threshold deve ser positivo");
        }

        let r = parse_rational(&args.r)?;
        let x0 = parse_rational(&args.x0)?;
        let alpha = parse_rational(&args.alpha)?;
        let innovation = parse_rational(&args.innovation)?;
        if x0.is_negative() || x0 > BigRational::one() {
            bail!("--x0 deve pertencer a [0,1]");
        }
        if r.is_negative() || r > BigRational::from_integer(4.into()) {
            bail!("--r deve pertencer a [0,4]");
        }

        Ok(Self {
            steps: args.steps,
            k,
            compress: args.compress,
            rounding: CircuitRounding::from(args.rounding),
            representation,
            workload,
            precision_bits,
            exponent_lanes,
            radius_bits,
            threshold,
            r,
            x0,
            alpha,
            innovation,
            output: args.output.clone(),
            export_proof: args.export_proof.clone(),
            verify_proof: args.verify_proof.clone(),
        })
    }

    fn fractional_bits(&self) -> usize {
        LIMB_WIDTH_BITS * self.k
    }

    fn fixed_scale(&self) -> BigUint {
        BigUint::one() << self.fractional_bits()
    }

    /// Numerador do limiar na grade de ponto fixo, exigindo representação exata.
    fn fixed_threshold_numerator(&self) -> Result<u128> {
        let scaled = &self.threshold * BigRational::from_integer(self.fixed_scale().into());
        if !scaled.is_integer() {
            bail!(
                "o limiar {} não é representável exatamente com {} bits fracionários; \
                 aumente --k ou escolha um limiar diádico",
                rational_text(&self.threshold),
                self.fractional_bits()
            );
        }
        let numerator = scaled
            .to_integer()
            .to_u128()
            .context("limiar de ponto fixo não cabe em u128")?;
        if numerator == 0 || BigUint::from(numerator) > self.fixed_scale() {
            bail!("o limiar deve pertencer a (0,1] no domínio de ponto fixo");
        }
        Ok(numerator)
    }

    /// Limiar na grade fina `2^64` da representação compensada.
    fn compensated_threshold_numerator(&self) -> Result<u128> {
        let fine_scale = BigUint::one() << 64_usize;
        let scaled = &self.threshold * BigRational::from_integer(fine_scale.clone().into());
        if !scaled.is_integer() {
            bail!(
                "o limiar {} não é representável exatamente na grade fina de 64 bits",
                rational_text(&self.threshold)
            );
        }
        let numerator = scaled
            .to_integer()
            .to_u128()
            .context("limiar compensado não cabe em u128")?;
        if numerator == 0 || BigUint::from(numerator) > fine_scale {
            bail!("o limiar deve pertencer a (0,1] no domínio compensado");
        }
        Ok(numerator)
    }

    fn r_parts(&self) -> Result<(u64, u64)> {
        small_ratio(&self.r, "r")
    }

    fn alpha_parts(&self) -> Result<(u64, u64)> {
        small_ratio(&self.alpha, "alpha")
    }

    fn innovation_parts(&self) -> Result<(u64, u64)> {
        small_ratio(&self.innovation, "innovation")
    }

    fn cfd_profile(&self) -> Result<CfdProfile> {
        let lanes = self.exponent_lanes.clone();
        let (minimum, maximum, operations, domain_max) = match self.workload {
            WorkloadArg::LogisticMap => (
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
                1,
            ),
            WorkloadArg::StableAffine => (
                BigRational::zero(),
                BigRational::from_integer(2.into()),
                vec![
                    CfdOperation::Add,
                    CfdOperation::Multiply,
                    CfdOperation::Fma,
                    CfdOperation::StableAffine,
                ],
                2,
            ),
        };
        if self.threshold > maximum {
            bail!(
                "o limiar {} excede o máximo {domain_max} do domínio CFD deste workload",
                rational_text(&self.threshold)
            );
        }
        let id = format!(
            "cfd-{}-p{}-e{}_{}-r{}",
            match self.workload {
                WorkloadArg::LogisticMap => "unit",
                WorkloadArg::StableAffine => "stable",
            },
            self.precision_bits,
            lanes.first().copied().unwrap_or_default(),
            lanes.last().copied().unwrap_or_default(),
            self.radius_bits,
        );
        CfdProfile::new(
            id,
            1,
            self.precision_bits,
            lanes,
            self.radius_bits,
            minimum,
            maximum,
            operations,
            vec![self.threshold.clone()],
            None,
        )
        .context("perfil CFD inválido para os parâmetros informados")
    }

    fn cfd_threshold_common(&self, profile: &CfdProfile) -> Result<u128> {
        common_threshold(profile, &self.threshold).with_context(|| {
            format!(
                "o limiar {} não é representável na escala diádica comum do perfil CFD",
                rational_text(&self.threshold)
            )
        })
    }

    fn fp32_threshold_biased(&self) -> Result<u8> {
        let threshold = self
            .threshold
            .to_f32()
            .context("limiar não converte para binary32")?;
        power_of_two_biased_exponent(threshold).with_context(|| {
            format!(
                "o gadget de decisão FP32 compara expoentes e exige limiar potência de dois; \
                 {} não é",
                rational_text(&self.threshold)
            )
        })
    }

    fn workload_spec(&self) -> WorkloadSpec {
        match self.workload {
            WorkloadArg::LogisticMap => WorkloadSpec::logistic_map(self.r.clone(), self.x0.clone()),
            WorkloadArg::StableAffine => WorkloadSpec::stable_affine(
                self.alpha.clone(),
                self.innovation.clone(),
                self.x0.clone(),
            ),
        }
    }

    /// Perfil canônico do ponto medido.
    ///
    /// Delega ao catálogo de `numeric-core` para que o binário de cobertura
    /// produza o mesmo `profile_hash_hex` a partir da mesma configuração.
    fn profile(&self, representation: RepresentationId) -> Result<ExperimentProfile> {
        FrontierPoint {
            representation,
            spec: self.workload_spec(),
            precision_bits: self.precision_bits,
            exponent_lanes: self.exponent_lanes.clone(),
            radius_bits: self.radius_bits,
            rounding: representation
                .rounding()
                .unwrap_or_else(|| rounding_id(self.rounding)),
            threshold: self.threshold.clone(),
        }
        .profile()
        .context("perfil de experimento inválido")
    }
}

#[derive(Debug, Serialize)]
struct Metrics {
    nova_snark_version: &'static str,
    curve_cycle: &'static str,
    commitment: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    proof_artifact_mode: Option<&'static str>,
    profile_hash_hex: String,
    workload: &'static str,
    p: usize,
    k: usize,
    precision_bits: usize,
    representation: &'static str,
    rounding: &'static str,
    threshold: String,
    r: String,
    x0: String,
    exponent_lanes: Vec<i16>,
    radius_bits: usize,
    steps: usize,
    primary_constraints: usize,
    secondary_constraints: usize,
    primary_variables: usize,
    secondary_variables: usize,
    raw_step_constraints: usize,
    nova_structural_overhead_constraints: usize,
    modeled_range_checks: usize,
    modeled_multiplication_relations: usize,
    modeled_division_relations: usize,
    modeled_canonicality: usize,
    modeled_counter: usize,
    setup_seconds: f64,
    prover_total_seconds: f64,
    prover_mean_step_seconds: f64,
    recursive_verify_seconds: f64,
    compression_setup_seconds: Option<f64>,
    compression_seconds: Option<f64>,
    compressed_verify_seconds: Option<f64>,
    compressed_proof_bytes_bincode: Option<usize>,
    state_field_elements: usize,
    peak_rss_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    isolated_mul_constraints: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    isolated_sub_one_constraints: Option<usize>,
}

/// Grandezas do backend que não dependem da representação medida.
struct IvcRun {
    primary_constraints: usize,
    secondary_constraints: usize,
    primary_variables: usize,
    secondary_variables: usize,
    setup_seconds: f64,
    prover_total_seconds: f64,
    recursive_verify_seconds: f64,
    compression_setup_seconds: Option<f64>,
    compression_seconds: Option<f64>,
    compressed_verify_seconds: Option<f64>,
    compressed_proof_bytes_bincode: Option<usize>,
}

/// Executa setup, dobra, verificação e compressão opcional.
///
/// Todas as representações passam por este caminho, de modo que diferenças
/// medidas venham do circuito e não da instrumentação.
fn run_ivc<C>(circuits: &[C], z0: &[Scalar], config: &Config) -> Result<IvcRun>
where
    C: StepCircuit<Scalar> + Clone,
{
    let steps=config.steps;
    let compress=config.compress || config.export_proof.is_some();
    let statement=serde_json::json!({"schema_version":1,"input":config.x0.to_string(),
        "r":config.r.to_string(),"alpha":config.alpha.to_string(),"innovation":config.innovation.to_string(),
        "horizon":steps,"threshold":config.threshold.to_string(),"precision":config.precision_bits,
        "family":format!("{:?}",config.representation),"rounding":config.rounding.label(),
        "workload":config.workload.label(),"exponent_lanes":config.exponent_lanes,"radius_bits":config.radius_bits,
        "input_contract":"public-canonical-v1","z0":z0});
    if let Some(path)=&config.export_proof {fs::create_dir(path).context("proof output must not exist")?;}
    if let Some(path)=&config.verify_proof {
        let supplied:serde_json::Value=serde_json::from_slice(&fs::read(path.join("statement.json"))?)?;
        if supplied!=statement {bail!("proof statement does not match authorized CLI input/profile/horizon");}
    }
    let started = Instant::now();
    let pp = PublicParams::<E1, E2, C>::setup(&circuits[0], &*S1::ck_floor(), &*S2::ck_floor())
        .context("PublicParams::setup")?;
    let setup_seconds = started.elapsed().as_secs_f64();

    if let Some(path)=&config.verify_proof {
        let started=Instant::now();
        let (_,vk)=CompressedSNARK::<E1,E2,C,S1,S2>::setup(&pp)?;
        let verifier_key_setup_seconds=started.elapsed().as_secs_f64();
        let started=Instant::now();
        let bytes=fs::read(path.join("proof.bin"))?;
        let (proof,used):(CompressedSNARK<E1,E2,C,S1,S2>,usize)=bincode::serde::decode_from_slice(&bytes,bincode::config::legacy())?;
        if used!=bytes.len() {bail!("trailing proof bytes");}
        let result=proof.verify(&vk,steps,z0).context("independent compressed verification")?;
        let claimed:Vec<Scalar>=serde_json::from_slice(&fs::read(path.join("terminal.json"))?)?;
        if result!=claimed {bail!("terminal state mismatch");}
        let verify_seconds=started.elapsed().as_secs_f64();
        let (primary_constraints,secondary_constraints)=pp.num_constraints();
        let (primary_variables,secondary_variables)=pp.num_variables();
        return Ok(IvcRun{primary_constraints,secondary_constraints,primary_variables,secondary_variables,
            setup_seconds,prover_total_seconds:0.0,recursive_verify_seconds:0.0,
            compression_setup_seconds:Some(verifier_key_setup_seconds),compression_seconds:None,
            compressed_verify_seconds:Some(verify_seconds),compressed_proof_bytes_bincode:Some(bytes.len())});
    }

    let mut recursive =
        RecursiveSNARK::<E1, E2, C>::new(&pp, &circuits[0], z0).context("RecursiveSNARK::new")?;
    let started = Instant::now();
    for circuit in circuits {
        recursive
            .prove_step(&pp, circuit)
            .context("RecursiveSNARK::prove_step")?;
    }
    let prover_total_seconds = started.elapsed().as_secs_f64();

    let started = Instant::now();
    recursive
        .verify(&pp, steps, z0)
        .context("RecursiveSNARK::verify")?;
    let recursive_verify_seconds = started.elapsed().as_secs_f64();

    let mut compression_setup_seconds = None;
    let mut compression_seconds = None;
    let mut compressed_verify_seconds = None;
    let mut compressed_proof_bytes_bincode = None;
    if compress {
        let started = Instant::now();
        let (pk, vk) =
            CompressedSNARK::<E1, E2, C, S1, S2>::setup(&pp).context("CompressedSNARK::setup")?;
        compression_setup_seconds = Some(started.elapsed().as_secs_f64());

        let started = Instant::now();
        let compressed = CompressedSNARK::<E1, E2, C, S1, S2>::prove(&pp, &pk, &recursive)
            .context("CompressedSNARK::prove")?;
        compression_seconds = Some(started.elapsed().as_secs_f64());
        let encoded = bincode::serde::encode_to_vec(&compressed, bincode::config::legacy())
            .context("serialização da prova comprimida")?;
        compressed_proof_bytes_bincode = Some(encoded.len());

        let started = Instant::now();
        let terminal = compressed
            .verify(&vk, steps, z0)
            .context("CompressedSNARK::verify")?;
        compressed_verify_seconds = Some(started.elapsed().as_secs_f64());
        if let Some(path)=&config.export_proof {
            fs::write(path.join("proof.bin"),&encoded)?;
            fs::write(path.join("statement.json"),serde_json::to_vec_pretty(&statement)?)?;
            fs::write(path.join("terminal.json"),serde_json::to_vec_pretty(&terminal)?)?;
        }
    }

    let (primary_constraints, secondary_constraints) = pp.num_constraints();
    let (primary_variables, secondary_variables) = pp.num_variables();
    Ok(IvcRun {
        primary_constraints,
        secondary_constraints,
        primary_variables,
        secondary_variables,
        setup_seconds,
        prover_total_seconds,
        recursive_verify_seconds,
        compression_setup_seconds,
        compression_seconds,
        compressed_verify_seconds,
        compressed_proof_bytes_bincode,
    })
}

struct Breakdown {
    range_checks: usize,
    multiplication_relations: usize,
    division_relations: usize,
    canonicality: usize,
    counter: usize,
}

#[allow(clippy::too_many_arguments)]
fn assemble(
    config: &Config,
    profile: &ExperimentProfile,
    representation: RepresentationId,
    rounding_label: &'static str,
    commitment: &'static str,
    legacy_p: usize,
    legacy_k: usize,
    state_field_elements: usize,
    raw_step_constraints: usize,
    breakdown: Breakdown,
    run: IvcRun,
    isolated_mul_constraints: Option<usize>,
    isolated_sub_one_constraints: Option<usize>,
) -> Metrics {
    Metrics {
        nova_snark_version: "0.73.0",
        curve_cycle: "Pallas/Vesta",
        commitment: if config.export_proof.is_some() || config.verify_proof.is_some() {"Public canonical boundary verification + Pedersen/IPA"} else {commitment},
        proof_artifact_mode: if config.verify_proof.is_some() {Some("independent-verify-only")} else if config.export_proof.is_some() {Some("export-compressed")} else {None},
        profile_hash_hex: profile.profile_hash_hex(),
        workload: WorkloadId::from(config.workload).tag(),
        p: legacy_p,
        k: legacy_k,
        precision_bits: config.precision_bits,
        representation: representation.tag(),
        rounding: rounding_label,
        threshold: rational_text(&config.threshold),
        r: rational_text(&config.r),
        x0: rational_text(&config.x0),
        exponent_lanes: if config.representation.family() == ProfileFamily::Cfd {
            config.exponent_lanes.clone()
        } else {
            Vec::new()
        },
        radius_bits: if config.representation.family() == ProfileFamily::Cfd {
            config.radius_bits
        } else {
            0
        },
        steps: config.steps,
        primary_constraints: run.primary_constraints,
        secondary_constraints: run.secondary_constraints,
        primary_variables: run.primary_variables,
        secondary_variables: run.secondary_variables,
        raw_step_constraints,
        nova_structural_overhead_constraints: run
            .primary_constraints
            .saturating_sub(raw_step_constraints),
        modeled_range_checks: breakdown.range_checks,
        modeled_multiplication_relations: breakdown.multiplication_relations,
        modeled_division_relations: breakdown.division_relations,
        modeled_canonicality: breakdown.canonicality,
        modeled_counter: breakdown.counter,
        setup_seconds: run.setup_seconds,
        prover_total_seconds: run.prover_total_seconds,
        prover_mean_step_seconds: run.prover_total_seconds / config.steps as f64,
        recursive_verify_seconds: run.recursive_verify_seconds,
        compression_setup_seconds: run.compression_setup_seconds,
        compression_seconds: run.compression_seconds,
        compressed_verify_seconds: run.compressed_verify_seconds,
        compressed_proof_bytes_bincode: run.compressed_proof_bytes_bincode,
        state_field_elements,
        peak_rss_bytes: peak_rss_bytes(),
        isolated_mul_constraints,
        isolated_sub_one_constraints,
    }
}

#[cfg(target_os = "macos")]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` points to valid writable memory for `getrusage`; the
    // value is only read after the call returns success.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if rc == 0 {
        // SAFETY: `getrusage` returned success and initialized `usage`.
        let usage = unsafe { usage.assume_init() };
        u64::try_from(usage.ru_maxrss).ok()
    } else {
        None
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` points to valid writable memory for `getrusage`; the
    // value is only read after the call returns success.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if rc == 0 {
        // SAFETY: `getrusage` returned success and initialized `usage`.
        let usage = unsafe { usage.assume_init() };
        u64::try_from(usage.ru_maxrss).ok().map(|kb| kb * 1024)
    } else {
        None
    }
}

#[cfg(not(unix))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

fn initial_limbs<const K: usize>(config: &Config, rounding: CircuitRounding) -> [u64; K] {
    initial_fixed_point_state::<K>(&config.x0, rounding).0
}

/// `z0` do ponto fixo: contador, limbs, `is_one` e a decisão do estado inicial.
fn fixed_initial_state_vector<const K: usize>(
    config: &Config,
    rounding: CircuitRounding,
) -> Result<Vec<u64>> {
    let (limbs, is_one) = initial_fixed_point_state::<K>(&config.x0, rounding);
    let numerator = if is_one {
        1_u128 << (16 * K)
    } else {
        limbs
            .iter()
            .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
    };
    let mut values = Vec::with_capacity(K + 3);
    values.push(0);
    values.extend(limbs);
    values.push(u64::from(is_one));
    values.push(u64::from(numerator >= config.fixed_threshold_numerator()?));
    Ok(values)
}

/// `z0` compensado: contador, limbs, `is_one`, residual e decisão inicial.
fn compensated_initial_state_vector(config: &Config) -> Result<Vec<u64>> {
    let (limbs, is_one, residual_offset) = initial_compensated_state(config);
    let (fine, _) = nova_circuit::quantize_dyadic(&config.x0, 64, config.rounding);
    Ok(vec![
        0,
        limbs[0],
        limbs[1],
        u64::from(is_one),
        residual_offset,
        u64::from(fine >= config.compensated_threshold_numerator()?),
    ])
}

fn circuits<const K: usize>(config: &Config) -> Result<Vec<LogisticStepCircuit<K>>> {
    let (r_num, r_den) = config.r_parts()?;
    let threshold_numerator = config.fixed_threshold_numerator()?;
    let rounding = config.rounding;
    let (mut limbs, mut is_one) = initial_fixed_point_state::<K>(&config.x0, rounding);
    let mut result = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = StepWitness::compute_with_rounding(limbs, is_one, r_num, r_den, rounding);
        limbs = witness.output_limbs;
        is_one = witness.output_is_one;
        result.push(LogisticStepCircuit {
            witness,
            r_num,
            r_den,
            rounding,
            threshold_numerator,
        });
    }
    Ok(result)
}

fn carry_circuits<const K: usize>(config: &Config) -> Result<Vec<CarryLogisticStepCircuit<K>>> {
    let (r_num, r_den) = config.r_parts()?;
    let threshold_numerator = config.fixed_threshold_numerator()?;
    let rounding = config.rounding;
    let mut limbs = initial_limbs::<K>(config, rounding);
    let mut is_one = false;
    let mut result = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = StepWitness::compute_with_rounding(limbs, is_one, r_num, r_den, rounding);
        let carry_witness = ProductCarryWitness::compute(limbs, is_one);
        limbs = witness.output_limbs;
        is_one = witness.output_is_one;
        result.push(CarryLogisticStepCircuit {
            witness,
            carry_witness,
            r_num,
            r_den,
            rounding,
            threshold_numerator,
        });
    }
    Ok(result)
}

fn initial_compensated_state(config: &Config) -> ([u64; 2], bool, u64) {
    nova_circuit::initial_compensated_state(&config.x0, config.rounding)
}

fn compensated_circuits(config: &Config) -> Result<Vec<CompensatedLogisticStepCircuit>> {
    let (r_num, r_den) = config.r_parts()?;
    let threshold_fine_numerator = config.compensated_threshold_numerator()?;
    let rounding = config.rounding;
    let (mut limbs, mut is_one, mut residual_offset) = initial_compensated_state(config);
    let mut result = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
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
        result.push(CompensatedLogisticStepCircuit {
            witness,
            r_num,
            r_den,
            rounding,
            threshold_fine_numerator,
        });
    }
    Ok(result)
}

fn initial_cbd_state<const K: usize>(config: &Config) -> ([u64; K], bool, u128) {
    nova_circuit::initial_cbd_state::<K>(&config.x0)
}

fn cbd_circuits<const K: usize>(
    config: &Config,
    radius_bound: CbdRadiusBound,
) -> Result<Vec<CbdLogisticStepCircuit<K>>> {
    let (r_num, r_den) = config.r_parts()?;
    let threshold_numerator = config.fixed_threshold_numerator()?;
    let (mut limbs, mut is_one, mut radius) = initial_cbd_state::<K>(config);
    let mut result = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = CbdStepWitness::compute_with_threshold(
            limbs,
            is_one,
            radius,
            r_num,
            r_den,
            radius_bound,
            threshold_numerator,
        );
        limbs = witness.output_limbs;
        is_one = witness.output_is_one;
        radius = witness.output_radius;
        result.push(CbdLogisticStepCircuit {
            witness,
            r_num,
            r_den,
            radius_bound,
            threshold_numerator,
        });
    }
    Ok(result)
}

fn cbd_sequential_circuits<const K: usize>(
    config: &Config,
) -> Result<Vec<CbdSequentialLogisticStepCircuit<K>>> {
    let (r_num, r_den) = config.r_parts()?;
    let threshold_numerator = config.fixed_threshold_numerator()?;
    let (mut limbs, mut is_one, mut radius) = initial_cbd_state::<K>(config);
    let mut result = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = CbdSequentialStepWitness::compute_with_threshold(
            limbs,
            is_one,
            radius,
            r_num,
            r_den,
            threshold_numerator,
        );
        limbs = witness.output_limbs;
        is_one = witness.output_is_one;
        radius = witness.output_radius;
        result.push(CbdSequentialLogisticStepCircuit {
            witness,
            r_num,
            r_den,
            threshold_numerator,
        });
    }
    Ok(result)
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

fn initial_fp32_state(config: &Config) -> Result<CircuitFp32> {
    let x0 = rational_to_f32(&config.x0, "x0")?;
    CircuitFp32::from_bits(x0.to_bits()).context("x0 não pertence ao perfil FP32 restrito")
}

fn fp32_circuits(config: &Config) -> Result<Vec<RestrictedFp32StepCircuit>> {
    let threshold_biased = config.fp32_threshold_biased()?;
    let parameter_bits = rational_to_f32(&config.r, "r")?.to_bits();
    let mut input_bits = rational_to_f32(&config.x0, "x0")?.to_bits();
    let mut result = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = RestrictedFp32StepWitness::compute_with_threshold(
            input_bits,
            parameter_bits,
            threshold_biased,
        )
        .context("testemunha da etapa FP32 restrita")?;
        input_bits = witness.output.bits();
        result.push(RestrictedFp32StepCircuit {
            witness,
            parameter_bits,
            threshold_biased_exponent: threshold_biased,
        });
    }
    Ok(result)
}

fn fp32_stable_circuits(config: &Config) -> Result<Vec<RestrictedFp32StableStepCircuit>> {
    let threshold_biased = config.fp32_threshold_biased()?;
    let alpha_bits = rational_to_f32(&config.alpha, "alpha")?.to_bits();
    let innovation_bits = rational_to_f32(&config.innovation, "innovation")?.to_bits();
    let mut input_bits = rational_to_f32(&config.x0, "x0")?.to_bits();
    let mut result = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = RestrictedFp32StableStepWitness::compute_with_threshold(
            input_bits,
            alpha_bits,
            innovation_bits,
            threshold_biased,
        )
        .context("testemunha da etapa FP32 estável")?;
        input_bits = witness.output.bits();
        result.push(RestrictedFp32StableStepCircuit {
            witness,
            alpha_bits,
            innovation_bits,
            threshold_biased_exponent: threshold_biased,
        });
    }
    Ok(result)
}

fn initial_cfd_state(config: &Config, profile: &CfdProfile) -> Result<CertifiedFloat> {
    CertifiedFloat::from_exact(profile.clone(), &config.x0)
        .context("x0 não pertence ao perfil CFD informado")
}

fn cfd_from_circuit_value(
    profile: &CfdProfile,
    value: &nova_circuit::CfdCircuitValue,
) -> CertifiedFloat {
    CertifiedFloat::from_parts(
        profile.clone(),
        false,
        value.exponent_lane as usize,
        BigUint::from(value.significand),
        BigUint::from(value.radius),
        value.zero,
        value.wide,
    )
    .expect("estado CFD circuital canônico")
}

fn cfd_circuits(
    config: &Config,
    profile: &CfdProfile,
    topology: CfdTopology,
) -> Result<Vec<CfdLogisticStepCircuit>> {
    let (r_num, r_den) = config.r_parts()?;
    let threshold_common = config.cfd_threshold_common(profile)?;
    let mut state = initial_cfd_state(config, profile)?;
    let mut result = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = CfdLogisticStepWitness::compute_with_threshold(
            profile,
            &state,
            r_num,
            r_den,
            topology,
            threshold_common,
        )
        .context("testemunha CFD")?;
        state = cfd_from_circuit_value(profile, &witness.output);
        result.push(CfdLogisticStepCircuit {
            profile: profile.clone(),
            witness,
            r_num,
            r_den,
            topology,
            threshold_common,
        });
    }
    Ok(result)
}

fn cfd_stable_circuits(config: &Config, profile: &CfdProfile) -> Result<Vec<CfdStableStepCircuit>> {
    let (alpha_num, alpha_den) = config.alpha_parts()?;
    let (innovation_num, innovation_den) = config.innovation_parts()?;
    let threshold_common = config.cfd_threshold_common(profile)?;
    let mut state = initial_cfd_state(config, profile)?;
    let mut result = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = CfdStableStepWitness::compute_with_threshold(
            profile,
            &state,
            alpha_num,
            alpha_den,
            innovation_num,
            innovation_den,
            threshold_common,
        )
        .context("testemunha CFD stable")?;
        state = cfd_from_circuit_value(profile, &witness.output);
        result.push(CfdStableStepCircuit {
            profile: profile.clone(),
            witness,
            alpha_num,
            alpha_den,
            innovation_num,
            innovation_den,
            threshold_common,
        });
    }
    Ok(result)
}

fn decision_code(decision: Decision3) -> u64 {
    match decision {
        Decision3::False => 0,
        Decision3::True => 1,
        Decision3::Indeterminate => 2,
    }
}

fn raw_step_constraints<const K: usize>(
    config: &Config,
    circuit: &LogisticStepCircuit<K>,
) -> usize {
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let values = fixed_initial_state_vector::<K>(config, circuit.rounding)
        .expect("limiar validado na resolução da configuração");
    let z = allocate_state(&mut cs, "z", &values);
    circuit
        .synthesize(&mut cs, &z)
        .expect("síntese da step circuit");
    cs.num_constraints()
}

fn raw_compensated_step_constraints(
    config: &Config,
    circuit: &CompensatedLogisticStepCircuit,
) -> usize {
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let values = compensated_initial_state_vector(config)
        .expect("limiar validado na resolução da configuração");
    let z = allocate_state(&mut cs, "z", &values);
    circuit
        .synthesize(&mut cs, &z)
        .expect("síntese da compensated step circuit");
    cs.num_constraints()
}

fn raw_carry_step_constraints<const K: usize>(
    config: &Config,
    circuit: &CarryLogisticStepCircuit<K>,
) -> usize {
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let values = fixed_initial_state_vector::<K>(config, circuit.rounding)
        .expect("limiar validado na resolução da configuração");
    let z = allocate_state(&mut cs, "z", &values);
    circuit
        .synthesize(&mut cs, &z)
        .expect("síntese da carry step circuit");
    cs.num_constraints()
}

fn allocate_state(
    cs: &mut TestConstraintSystem<Scalar>,
    prefix: &'static str,
    values: &[u64],
) -> Vec<AllocatedNum<Scalar>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cs.namespace(|| format!("{prefix}_{index}")), || {
                Ok(Scalar::from(*value))
            })
            .expect("alocação válida")
        })
        .collect()
}

fn allocate_wide_state(
    cs: &mut TestConstraintSystem<Scalar>,
    prefix: &'static str,
    values: &[u128],
) -> Vec<AllocatedNum<Scalar>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cs.namespace(|| format!("{prefix}_{index}")), || {
                Ok(field_from_u128_main::<Scalar>(*value))
            })
            .expect("alocação válida")
        })
        .collect()
}

fn cbd_initial_state_vector<const K: usize>(config: &Config) -> Vec<u128> {
    let (limbs, is_one, radius) = initial_cbd_state::<K>(config);
    let mut values = Vec::with_capacity(K + 4);
    values.push(0);
    values.extend(limbs.into_iter().map(u128::from));
    values.push(u128::from(is_one));
    values.push(radius);
    values.push(0);
    values
}

fn raw_cbd_step_constraints<const K: usize>(
    config: &Config,
    circuit: &CbdLogisticStepCircuit<K>,
) -> usize {
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let z = allocate_wide_state(&mut cs, "cbd_z", &cbd_initial_state_vector::<K>(config));
    circuit
        .synthesize(&mut cs, &z)
        .expect("síntese da CBD step circuit");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    cs.num_constraints()
}

fn raw_cbd_sequential_step_constraints<const K: usize>(
    config: &Config,
    circuit: &CbdSequentialLogisticStepCircuit<K>,
) -> usize {
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let z = allocate_wide_state(&mut cs, "cbd_seq_z", &cbd_initial_state_vector::<K>(config));
    circuit
        .synthesize(&mut cs, &z)
        .expect("síntese da CBD sequencial");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    cs.num_constraints()
}

fn cfd_initial_state_vector(
    profile: &CfdProfile,
    input: &CertifiedFloat,
    threshold_common: u128,
) -> Result<Vec<u128>> {
    Ok(vec![
        0,
        input.exponent_lane() as u128,
        input.significand().to_u128().context("significando CFD")?,
        input.radius().to_u128().context("raio CFD")?,
        u128::from(input.is_zero()),
        u128::from(input.is_wide()),
        u128::from(decision_code(circuit_decision(
            profile,
            input,
            threshold_common,
        )?)),
    ])
}

fn raw_cfd_step_constraints(circuit: &CfdLogisticStepCircuit, state: &[u128]) -> usize {
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let z = allocate_wide_state(&mut cs, "cfd_z", state);
    circuit
        .synthesize(&mut cs, &z)
        .expect("síntese da step circuit CFD");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    cs.num_constraints()
}

fn raw_cfd_stable_step_constraints(circuit: &CfdStableStepCircuit, state: &[u128]) -> usize {
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let z = allocate_wide_state(&mut cs, "stable_z", state);
    circuit.synthesize(&mut cs, &z).expect("síntese stable");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    cs.num_constraints()
}

fn fp32_state_vector(value: CircuitFp32) -> Vec<u64> {
    vec![
        0,
        value.exponent as u64,
        value.significand as u64,
        value.is_zero as u64,
        0,
    ]
}

fn raw_fp32_step_constraints(circuit: &RestrictedFp32StepCircuit, initial: CircuitFp32) -> usize {
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let z = allocate_state(&mut cs, "fp32_step_z", &fp32_state_vector(initial));
    circuit
        .synthesize(&mut cs, &z)
        .expect("síntese da step circuit FP32");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    cs.num_constraints()
}

fn raw_fp32_stable_step_constraints(
    circuit: &RestrictedFp32StableStepCircuit,
    initial: CircuitFp32,
) -> usize {
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let z = allocate_state(&mut cs, "fp32_stable_z", &fp32_state_vector(initial));
    circuit
        .synthesize(&mut cs, &z)
        .expect("síntese da step circuit FP32 estável");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    cs.num_constraints()
}

fn raw_fp32_mul_constraints() -> usize {
    let witness =
        Fp32MulWitness::compute(0.1_f32.to_bits(), 0.9_f32.to_bits()).expect("multiplicação FP32");
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let values = [
        0,
        witness.left.exponent as u64,
        witness.left.significand as u64,
        witness.left.is_zero as u64,
        witness.right.exponent as u64,
        witness.right.significand as u64,
        witness.right.is_zero as u64,
    ];
    let z = allocate_state(&mut cs, "fp32_mul_z", &values);
    Fp32MulCircuit { witness }
        .synthesize(&mut cs, &z)
        .expect("síntese da multiplicação FP32");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    cs.num_constraints()
}

fn raw_fp32_sub_one_constraints() -> usize {
    let witness = Fp32SubOneWitness::compute(0.1_f32.to_bits()).expect("subtração FP32");
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let values = [
        0,
        witness.input.exponent as u64,
        witness.input.significand as u64,
        witness.input.is_zero as u64,
    ];
    let z = allocate_state(&mut cs, "fp32_sub_z", &values);
    Fp32SubOneCircuit { witness }
        .synthesize(&mut cs, &z)
        .expect("síntese da subtração FP32");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    cs.num_constraints()
}

fn field_from_u128_main<F: ff::PrimeField>(value: u128) -> F {
    let high = F::from((value >> 64) as u64);
    let mut two64 = F::ONE;
    for _ in 0..64 {
        two64 = two64.double();
    }
    high * two64 + F::from(value as u64)
}

fn execute<const K: usize>(config: &Config) -> Result<Metrics> {
    let circuits = circuits::<K>(config)?;
    let z0 = fixed_initial_state_vector::<K>(config, config.rounding)?
        .into_iter()
        .map(Scalar::from)
        .collect::<Vec<_>>();

    let run = run_ivc(&circuits, &z0, config)?;
    let raw = raw_step_constraints(config, &circuits[0]);
    let breakdown = circuits[0].breakdown();
    let profile = config.profile(RepresentationId::Standard)?;
    Ok(assemble(
        config,
        &profile,
        RepresentationId::Standard,
        config.rounding.label(),
        "Pedersen + IPA",
        LIMB_WIDTH_BITS,
        K,
        LogisticStepCircuit::<K>::arity_value(),
        raw,
        Breakdown {
            range_checks: breakdown.range_checks,
            multiplication_relations: breakdown.multiplication_relations,
            division_relations: breakdown.division_relations,
            canonicality: breakdown.canonicality,
            counter: breakdown.counter,
        },
        run,
        None,
        None,
    ))
}

fn execute_carry<const K: usize>(config: &Config) -> Result<Metrics> {
    let circuits = carry_circuits::<K>(config)?;
    let z0 = fixed_initial_state_vector::<K>(config, config.rounding)?
        .into_iter()
        .map(Scalar::from)
        .collect::<Vec<_>>();

    let run = run_ivc(&circuits, &z0, config)?;
    let raw = raw_carry_step_constraints(config, &circuits[0]);
    let multiplication_relations = K * K;
    let division_relations = 2;
    let canonicality = 3 * (K + 1);
    let counter = 1;
    let profile = config.profile(RepresentationId::Carry)?;
    Ok(assemble(
        config,
        &profile,
        RepresentationId::Carry,
        config.rounding.label(),
        "Pedersen + IPA",
        LIMB_WIDTH_BITS,
        K,
        CarryLogisticStepCircuit::<K>::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(
                multiplication_relations + division_relations + canonicality + counter,
            ),
            multiplication_relations,
            division_relations,
            canonicality,
            counter,
        },
        run,
        None,
        None,
    ))
}

fn execute_compensated(config: &Config) -> Result<Metrics> {
    let circuits = compensated_circuits(config)?;
    let z0 = compensated_initial_state_vector(config)?
        .into_iter()
        .map(Scalar::from)
        .collect::<Vec<_>>();

    let run = run_ivc(&circuits, &z0, config)?;
    let raw = raw_compensated_step_constraints(config, &circuits[0]);
    let multiplication_relations = 2;
    let division_relations = 1;
    let canonicality = 6;
    let counter = 1;
    let profile = config.profile(RepresentationId::Compensated)?;
    Ok(assemble(
        config,
        &profile,
        RepresentationId::Compensated,
        match config.rounding {
            CircuitRounding::Truncate => "truncate_fine_grid",
            CircuitRounding::NearestTiesToEven => "nearest_ties_to_even_fine_grid",
        },
        "Pedersen + IPA",
        LIMB_WIDTH_BITS,
        2,
        CompensatedLogisticStepCircuit::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(
                multiplication_relations + division_relations + canonicality + counter,
            ),
            multiplication_relations,
            division_relations,
            canonicality,
            counter,
        },
        run,
        None,
        None,
    ))
}

/// Parâmetros afins validados: o atrator precisa caber no domínio `[0,2)`.
///
/// A recorrência converge para `c/(1-alpha)`. Se esse valor passar de 2, a
/// trajetória estoura o estado no meio da varredura e o circuito falha com
/// assert em vez de mensagem.
fn affine_parameters(config: &Config) -> Result<AffineParameters> {
    let (alpha_num, alpha_den) = config.alpha_parts()?;
    let (innovation_num, innovation_den) = config.innovation_parts()?;
    if alpha_num >= alpha_den {
        bail!("--alpha deve pertencer a (0,1) para a recorrência ser contrativa");
    }
    if alpha_num == 0 {
        bail!("--alpha nulo elimina a recorrência");
    }
    for (name, value) in [
        ("alpha", alpha_num.max(alpha_den)),
        ("innovation", innovation_num.max(innovation_den)),
    ] {
        if value >= 1 << 16 {
            bail!("--{name} precisa de numerador e denominador abaixo de 2^16");
        }
    }
    let parameters = AffineParameters::new(alpha_num, alpha_den, innovation_num, innovation_den);
    let (attractor_num, attractor_den) = parameters.attractor();
    if attractor_num >= 2 * attractor_den {
        bail!(
            "o atrator {attractor_num}/{attractor_den} da recorrência afim sai do domínio [0,2); \
             reduza --innovation ou --alpha"
        );
    }
    Ok(parameters)
}

/// Estado inicial afim: `x0` na grade, com o bit alto livre em vez de indicar
/// `x == 1`.
fn affine_initial_state<const K: usize>(config: &Config) -> Result<(u128, [u64; K], bool)> {
    let scale = 1_u128 << (LIMB_WIDTH_BITS * K);
    let scaled = &config.x0 * BigRational::from_integer(BigInt::from(scale));
    let value = (scaled.numer() / scaled.denom())
        .to_u128()
        .context("--x0 fora da faixa representável")?;
    if value >= 2 * scale {
        bail!("--x0 fora do domínio [0,2) do workload afim");
    }
    let high = value >= scale;
    let mut remaining = value - if high { scale } else { 0 };
    let mut limbs = [0_u64; K];
    for limb in limbs.iter_mut().rev() {
        *limb = (remaining & 0xffff) as u64;
        remaining >>= 16;
    }
    Ok((value, limbs, high))
}

/// Numerador do limiar afim na escala `2^(16K)`, aceitando `[0,2)`.
fn affine_threshold_numerator<const K: usize>(config: &Config) -> Result<u128> {
    let scale = 1_u128 << (LIMB_WIDTH_BITS * K);
    let scaled = &config.threshold * BigRational::from_integer(BigInt::from(scale));
    if !scaled.is_integer() {
        bail!(
            "--threshold {} não é representável na grade de {} bits",
            config.threshold,
            LIMB_WIDTH_BITS * K
        );
    }
    let value = scaled
        .to_integer()
        .to_u128()
        .context("--threshold fora da faixa representável")?;
    if value == 0 || value >= 2 * scale {
        bail!("--threshold do workload afim deve pertencer a (0,2)");
    }
    Ok(value)
}

fn execute_affine<const K: usize>(config: &Config) -> Result<Metrics> {
    let parameters = affine_parameters(config)?;
    let threshold_numerator = affine_threshold_numerator::<K>(config)?;
    let (_, mut limbs, mut high) = affine_initial_state::<K>(config)?;

    let mut z0 = Vec::with_capacity(K + 3);
    z0.push(0_u128);
    z0.extend(limbs.into_iter().map(u128::from));
    z0.push(u128::from(high));
    z0.push(0);

    let mut circuits = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = AffineStepWitness::<K>::compute(limbs, high, parameters, config.rounding);
        limbs = witness.output_limbs;
        high = witness.output_high;
        circuits.push(AffineStepCircuit::<K> {
            witness,
            parameters,
            rounding: config.rounding,
            threshold_numerator,
        });
    }

    let scalars = z0
        .iter()
        .map(|value| field_from_u128_main::<Scalar>(*value))
        .collect::<Vec<_>>();
    let run = run_ivc(&circuits, &scalars, config)?;

    let mut cs = TestConstraintSystem::<Scalar>::new();
    let z = allocate_wide_state(&mut cs, "affine_z", &z0);
    circuits[0]
        .synthesize(&mut cs, &z)
        .expect("síntese da etapa afim");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    let raw = cs.num_constraints();

    // A etapa afim não multiplica duas variáveis: `alpha*x + c` é combinação
    // linear. O único produto é o do gadget de arredondamento.
    let multiplication_relations = 0;
    let division_relations = 2;
    let canonicality = 2 * (K + 1) + 2;
    let counter = 1;
    let representation = RepresentationId::Standard;
    let profile = config.profile(representation)?;
    Ok(assemble(
        config,
        &profile,
        representation,
        config.rounding.label(),
        "Pedersen + IPA",
        LIMB_WIDTH_BITS,
        K,
        AffineStepCircuit::<K>::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(
                multiplication_relations + division_relations + canonicality + counter,
            ),
            multiplication_relations,
            division_relations,
            canonicality,
            counter,
        },
        run,
        None,
        None,
    ))
}

fn execute_cbd_affine<const K: usize>(config: &Config) -> Result<Metrics> {
    let parameters = affine_parameters(config)?;
    let threshold_numerator = affine_threshold_numerator::<K>(config)?;
    let (value, mut limbs, mut high) = affine_initial_state::<K>(config)?;
    // Mesma convenção do CBD logístico: raio de uma ULP quando `x0` não cai na
    // grade, zero quando cai.
    let scale = 1_u128 << (LIMB_WIDTH_BITS * K);
    let exact = &config.x0 * BigRational::from_integer(BigInt::from(scale));
    let mut radius = u128::from(!exact.is_integer());
    let _ = value;

    let mut z0 = Vec::with_capacity(K + 4);
    z0.push(0_u128);
    z0.extend(limbs.into_iter().map(u128::from));
    z0.push(u128::from(high));
    z0.push(radius);
    z0.push(0);

    let mut circuits = Vec::with_capacity(config.steps);
    for _ in 0..config.steps {
        let witness = CbdAffineStepWitness::<K>::compute(
            limbs,
            high,
            radius,
            parameters,
            config.rounding,
            threshold_numerator,
        );
        limbs = witness.output_limbs;
        high = witness.output_high;
        radius = witness.output_radius;
        circuits.push(CbdAffineStepCircuit::<K> {
            witness,
            parameters,
            rounding: config.rounding,
            threshold_numerator,
        });
    }

    let scalars = z0
        .iter()
        .map(|value| field_from_u128_main::<Scalar>(*value))
        .collect::<Vec<_>>();
    let run = run_ivc(&circuits, &scalars, config)?;

    let mut cs = TestConstraintSystem::<Scalar>::new();
    let z = allocate_wide_state(&mut cs, "cbd_affine_z", &z0);
    circuits[0]
        .synthesize(&mut cs, &z)
        .expect("síntese da etapa afim certificada");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    let raw = cs.num_constraints();

    let multiplication_relations = 2;
    let division_relations = 4;
    let canonicality = 2 * (K + 1) + 10;
    let counter = 1;
    let representation = RepresentationId::CbdLocalTaylor;
    let profile = config.profile(representation)?;
    Ok(assemble(
        config,
        &profile,
        representation,
        "nearest_ties_to_even_affine",
        "Pedersen + IPA",
        LIMB_WIDTH_BITS,
        K,
        CbdAffineStepCircuit::<K>::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(
                multiplication_relations + division_relations + canonicality + counter,
            ),
            multiplication_relations,
            division_relations,
            canonicality,
            counter,
        },
        run,
        None,
        None,
    ))
}

fn execute_cbd<const K: usize>(config: &Config, radius_bound: CbdRadiusBound) -> Result<Metrics> {
    let circuits = cbd_circuits::<K>(config, radius_bound)?;
    let z0 = cbd_initial_state_vector::<K>(config)
        .into_iter()
        .map(field_from_u128_main::<Scalar>)
        .collect::<Vec<_>>();

    let run = run_ivc(&circuits, &z0, config)?;
    let raw = raw_cbd_step_constraints(config, &circuits[0]);
    let multiplication_relations = match radius_bound {
        CbdRadiusBound::Global => 1,
        CbdRadiusBound::LocalTaylor => 3,
    };
    let division_relations = 2;
    let canonicality = 2 * (K + 1) + 8;
    let counter = 1;
    let representation = match radius_bound {
        CbdRadiusBound::Global => RepresentationId::CbdLipschitz,
        CbdRadiusBound::LocalTaylor => RepresentationId::CbdLocalTaylor,
    };
    let profile = config.profile(representation)?;
    Ok(assemble(
        config,
        &profile,
        representation,
        "nearest_ties_to_even_fused",
        "Pedersen + IPA",
        LIMB_WIDTH_BITS,
        K,
        CbdLogisticStepCircuit::<K>::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(
                multiplication_relations + division_relations + canonicality + counter,
            ),
            multiplication_relations,
            division_relations,
            canonicality,
            counter,
        },
        run,
        None,
        None,
    ))
}

fn execute_cdad<const K: usize>(config: &Config) -> Result<Metrics> {
    let inner = cbd_circuits::<K>(config, CbdRadiusBound::LocalTaylor)?;
    let circuits = inner
        .into_iter()
        .map(CdadFixedLogisticStepCircuit::new)
        .collect::<Vec<_>>();
    let mut z0_values = cbd_initial_state_vector::<K>(config);
    z0_values.push((16 * K) as u128);
    let z0 = z0_values
        .iter()
        .copied()
        .map(field_from_u128_main::<Scalar>)
        .collect::<Vec<_>>();
    let run = run_ivc(&circuits, &z0, config)?;

    let mut cs = TestConstraintSystem::<Scalar>::new();
    let z = allocate_wide_state(&mut cs, "cdad_z", &z0_values);
    circuits[0]
        .synthesize(&mut cs, &z)
        .expect("síntese CDAD fixa");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    let raw = cs.num_constraints();
    let multiplication_relations = 3;
    let division_relations = 2;
    let canonicality = 2 * (K + 1) + 10;
    let counter = 1;
    let representation = RepresentationId::CdadFixed;
    let profile = config.profile(representation)?;
    Ok(assemble(
        config,
        &profile,
        representation,
        "nearest_ties_to_even_decision_adaptive_fixed",
        "Pedersen + IPA; Poseidon link between profiles",
        LIMB_WIDTH_BITS,
        K,
        CdadFixedLogisticStepCircuit::<K>::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(
                multiplication_relations + division_relations + canonicality + counter,
            ),
            multiplication_relations,
            division_relations,
            canonicality,
            counter,
        },
        run,
        None,
        None,
    ))
}

fn execute_cbd_sequential<const K: usize>(config: &Config) -> Result<Metrics> {
    let circuits = cbd_sequential_circuits::<K>(config)?;
    let z0 = cbd_initial_state_vector::<K>(config)
        .into_iter()
        .map(field_from_u128_main::<Scalar>)
        .collect::<Vec<_>>();

    let run = run_ivc(&circuits, &z0, config)?;
    let raw = raw_cbd_sequential_step_constraints(config, &circuits[0]);
    let multiplication_relations = 3;
    let division_relations = 4;
    let canonicality = 3 * (K + 1) + 12;
    let counter = 1;
    let profile = config.profile(RepresentationId::CbdSequentialLocalTaylor)?;
    Ok(assemble(
        config,
        &profile,
        RepresentationId::CbdSequentialLocalTaylor,
        "nearest_ties_to_even_sequential",
        "Pedersen + IPA",
        LIMB_WIDTH_BITS,
        K,
        CbdSequentialLogisticStepCircuit::<K>::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(
                multiplication_relations + division_relations + canonicality + counter,
            ),
            multiplication_relations,
            division_relations,
            canonicality,
            counter,
        },
        run,
        None,
        None,
    ))
}

fn execute_cfd(config: &Config, topology: CfdTopology) -> Result<Metrics> {
    let profile = config.cfd_profile()?;
    let threshold_common = config.cfd_threshold_common(&profile)?;
    let circuits = cfd_circuits(config, &profile, topology)?;
    let initial = initial_cfd_state(config, &profile)?;
    let state = cfd_initial_state_vector(&profile, &initial, threshold_common)?;
    let z0 = state
        .iter()
        .copied()
        .map(field_from_u128_main::<Scalar>)
        .collect::<Vec<_>>();

    let run = run_ivc(&circuits, &z0, config)?;
    let raw = raw_cfd_step_constraints(&circuits[0], &state);
    let multiplication_relations = match topology {
        CfdTopology::Sequential => 12,
        CfdTopology::Fused => 8,
    };
    let canonicality = 18;
    let counter = 1;
    let representation = match topology {
        CfdTopology::Sequential => RepresentationId::CfdSequential,
        CfdTopology::Fused => RepresentationId::CfdFused,
    };
    let experiment_profile = config.profile(representation)?;
    Ok(assemble(
        config,
        &experiment_profile,
        representation,
        "honest_rne_certified_enclosure",
        "Pedersen + IPA; SHA-256 profile manifest",
        config.precision_bits,
        profile.exponent_lanes().len(),
        CfdLogisticStepCircuit::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(multiplication_relations + canonicality + counter),
            multiplication_relations,
            division_relations: 0,
            canonicality,
            counter,
        },
        run,
        None,
        None,
    ))
}

fn execute_cfd_stable(config: &Config) -> Result<Metrics> {
    let profile = config.cfd_profile()?;
    let threshold_common = config.cfd_threshold_common(&profile)?;
    let circuits = cfd_stable_circuits(config, &profile)?;
    let initial = initial_cfd_state(config, &profile)?;
    let state = cfd_initial_state_vector(&profile, &initial, threshold_common)?;
    let z0 = state
        .iter()
        .copied()
        .map(field_from_u128_main::<Scalar>)
        .collect::<Vec<_>>();

    let run = run_ivc(&circuits, &z0, config)?;
    let raw = raw_cfd_stable_step_constraints(&circuits[0], &state);
    let multiplication_relations = 6;
    let canonicality = 14;
    let counter = 1;
    let experiment_profile = config.profile(RepresentationId::CfdSequential)?;
    Ok(assemble(
        config,
        &experiment_profile,
        RepresentationId::CfdSequential,
        "honest_rne_certified_enclosure",
        "Pedersen + IPA; SHA-256 profile manifest",
        config.precision_bits,
        profile.exponent_lanes().len(),
        CfdStableStepCircuit::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(multiplication_relations + canonicality + counter),
            multiplication_relations,
            division_relations: 0,
            canonicality,
            counter,
        },
        run,
        None,
        None,
    ))
}

fn execute_fp32(config: &Config) -> Result<Metrics> {
    let circuits = fp32_circuits(config)?;
    let initial = initial_fp32_state(config)?;
    let z0 = fp32_state_vector(initial)
        .into_iter()
        .map(Scalar::from)
        .collect::<Vec<_>>();

    let run = run_ivc(&circuits, &z0, config)?;
    let raw = raw_fp32_step_constraints(&circuits[0], initial);
    let range_checks = RestrictedFp32StepCircuit::range_check_constraints();
    let multiplication_relations = 6;
    let division_relations = 3;
    let counter = 1;
    let profile = config.profile(RepresentationId::Fp32Exact)?;
    Ok(assemble(
        config,
        &profile,
        RepresentationId::Fp32Exact,
        "nearest_ties_to_even_separated",
        "Pedersen + IPA",
        FP32_PRECISION_BITS,
        1,
        RestrictedFp32StepCircuit::arity_value(),
        raw,
        Breakdown {
            range_checks,
            multiplication_relations,
            division_relations,
            canonicality: raw.saturating_sub(
                range_checks + multiplication_relations + division_relations + counter,
            ),
            counter,
        },
        run,
        Some(raw_fp32_mul_constraints()),
        Some(raw_fp32_sub_one_constraints()),
    ))
}

fn execute_fp32_stable(config: &Config) -> Result<Metrics> {
    let circuits = fp32_stable_circuits(config)?;
    let initial = initial_fp32_state(config)?;
    let z0 = fp32_state_vector(initial)
        .into_iter()
        .map(Scalar::from)
        .collect::<Vec<_>>();

    let run = run_ivc(&circuits, &z0, config)?;
    let raw = raw_fp32_stable_step_constraints(&circuits[0], initial);
    let profile = config.profile(RepresentationId::Fp32Exact)?;
    Ok(assemble(
        config,
        &profile,
        RepresentationId::Fp32Exact,
        "nearest_ties_to_even_separated",
        "Pedersen + IPA",
        FP32_PRECISION_BITS,
        1,
        RestrictedFp32StableStepCircuit::arity_value(),
        raw,
        Breakdown {
            range_checks: raw.saturating_sub(12),
            multiplication_relations: 3,
            division_relations: 2,
            canonicality: 6,
            counter: 1,
        },
        run,
        Some(raw_fp32_mul_constraints()),
        None,
    ))
}

fn rounding_id(rounding: CircuitRounding) -> RoundingId {
    match rounding {
        CircuitRounding::Truncate => RoundingId::Truncate,
        CircuitRounding::NearestTiesToEven => RoundingId::NearestTiesToEven,
    }
}

fn require_nearest_even(config: &Config, family: &str) -> Result<()> {
    if config.rounding != CircuitRounding::NearestTiesToEven {
        bail!("{family} exige --rounding nearest-even");
    }
    Ok(())
}

fn require_logistic(config: &Config, family: &str) -> Result<()> {
    if config.workload != WorkloadArg::LogisticMap {
        bail!(
            "{family} só possui step circuit para --workload logistic-map; \
             a cobertura do workload afim estável é medida fora de circuito \
             por frontier-eval"
        );
    }
    Ok(())
}

fn run(config: &Config) -> Result<Metrics> {
    match config.representation {
        RepresentationArg::Standard if config.workload == WorkloadArg::StableAffine => {
            match config.k {
                1 => execute_affine::<1>(config),
                2 => execute_affine::<2>(config),
                3 => execute_affine::<3>(config),
                4 => execute_affine::<4>(config),
                _ => unreachable!("--k é validado em 1..=4"),
            }
        }
        RepresentationArg::Cbd
        | RepresentationArg::CbdGlobal
        | RepresentationArg::CbdSequential
            if config.workload == WorkloadArg::StableAffine =>
        {
            require_nearest_even(config, "CBD")?;
            // A recorrência afim tem um único bound de raio: `alpha` é o próprio
            // fator de Lipschitz e é exato. Lipschitz global, Taylor local e
            // sequencial coincidem, e emitir três rótulos para o mesmo circuito
            // criaria três pontos idênticos na fronteira.
            if config.representation != RepresentationArg::Cbd {
                bail!(
                    "no workload afim o raio certificado é exato e os três bound modes \
                     coincidem; use --representation cbd"
                );
            }
            match config.k {
                1 => execute_cbd_affine::<1>(config),
                2 => execute_cbd_affine::<2>(config),
                3 => execute_cbd_affine::<3>(config),
                4 => execute_cbd_affine::<4>(config),
                _ => unreachable!("--k é validado em 1..=4"),
            }
        }
        RepresentationArg::Carry if config.workload == WorkloadArg::StableAffine => {
            bail!(
                "a decomposição de carry existe para o produto `x*(1-x)`; a etapa afim \
                 não tem produto de duas variáveis e coincide com --representation standard"
            )
        }
        RepresentationArg::Standard => match config.k {
            1 => execute::<1>(config),
            2 => execute::<2>(config),
            3 => execute::<3>(config),
            4 => execute::<4>(config),
            _ => unreachable!("--k é validado em 1..=4"),
        },
        RepresentationArg::Carry => match config.k {
            1 => execute_carry::<1>(config),
            2 => execute_carry::<2>(config),
            3 => execute_carry::<3>(config),
            4 => execute_carry::<4>(config),
            _ => unreachable!("--k é validado em 1..=4"),
        },
        RepresentationArg::Compensated => {
            if config.k != 2 {
                bail!("a representação compensada está implementada apenas para --k 2");
            }
            execute_compensated(config)
        }
        RepresentationArg::Cbd | RepresentationArg::CbdGlobal => {
            require_nearest_even(config, "CBD")?;
            let bound = if config.representation == RepresentationArg::Cbd {
                CbdRadiusBound::LocalTaylor
            } else {
                CbdRadiusBound::Global
            };
            match config.k {
                1 => execute_cbd::<1>(config, bound),
                2 => execute_cbd::<2>(config, bound),
                3 => execute_cbd::<3>(config, bound),
                4 => execute_cbd::<4>(config, bound),
                _ => unreachable!("--k é validado em 1..=4"),
            }
        }
        RepresentationArg::CbdSequential => {
            require_nearest_even(config, "CBD sequencial")?;
            match config.k {
                1 => execute_cbd_sequential::<1>(config),
                2 => execute_cbd_sequential::<2>(config),
                3 => execute_cbd_sequential::<3>(config),
                4 => execute_cbd_sequential::<4>(config),
                _ => unreachable!("--k é validado em 1..=4"),
            }
        }
        RepresentationArg::Cdad => {
            require_nearest_even(config, "CDAD")?;
            if config.workload != WorkloadArg::LogisticMap {
                bail!("o protótipo Nova CDAD v1 implementa o mapa logístico");
            }
            match config.k {
                1 => execute_cdad::<1>(config),
                2 => execute_cdad::<2>(config),
                3 => execute_cdad::<3>(config),
                4 => execute_cdad::<4>(config),
                _ => unreachable!("--k é validado em 1..=4"),
            }
        }
        RepresentationArg::Cfd | RepresentationArg::CfdFused | RepresentationArg::CfdStable => {
            require_nearest_even(config, "CFD")?;
            if config.k != 1 {
                bail!("CFD usa --k 1; a largura do corredor vem de --exponent-lanes");
            }
            match config.workload {
                WorkloadArg::StableAffine => execute_cfd_stable(config),
                WorkloadArg::LogisticMap => execute_cfd(
                    config,
                    if config.representation == RepresentationArg::CfdFused {
                        CfdTopology::Fused
                    } else {
                        CfdTopology::Sequential
                    },
                ),
            }
        }
        RepresentationArg::Fp32 | RepresentationArg::Fp32Stable => {
            require_nearest_even(config, "FP32 restrito")?;
            match config.workload {
                WorkloadArg::StableAffine => execute_fp32_stable(config),
                WorkloadArg::LogisticMap => execute_fp32(config),
            }
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::resolve(&args)?;
    if (config.export_proof.is_some() || config.verify_proof.is_some()) && config.output.exists() {
        bail!("proof artifact metrics output must not already exist");
    }
    if config.representation == RepresentationArg::Compensated {
        require_logistic(&config, "a representação compensada")?;
    }
    let metrics = run(&config)?;
    if let Some(parent) = config.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&config.output, serde_json::to_vec_pretty(&metrics)?)?;
    println!("{}", serde_json::to_string_pretty(&metrics)?);
    Ok(())
}

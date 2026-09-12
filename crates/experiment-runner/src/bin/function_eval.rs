use anyhow::{bail, Context, Result};
use clap::Parser;
use csv::Writer;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use numeric_approximations::{
    evaluate_gauss_legendre_prepared, evaluate_pade, evaluate_taylor, prepare_gauss_legendre,
    GaussLegendreEvaluation, OperationCounts, PolynomialEvaluation, PreparedGaussLegendre,
};
use numeric_core::{
    ApproximationMethod, ApproximationSpec, BoundRule, CompositionRule, DecisionCertification,
    ErrorComponent, ErrorContract, EvidenceLevel, ExceptionalValuePolicy, FixedPointFormat,
    FunctionId, NumericContractProfile, NumericContractProfileBuilder, NumericRepresentationKind,
    OperationSemantics, OverflowMode, ProfileProvenance, QuantizationRounding, RationalSpec,
    RepresentationSpec, RescalingMode, SemanticClass, SourceKind, UnderflowMode, ValidDomain,
};
use numeric_reference::{
    evaluate_transcendental, float_to_big_rational, rational_to_float,
    resolve_transcendental_decision,
};
use rug::Float;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

const RESULT_SCHEMA_VERSION: u16 = 2;
const DEFAULT_SEED: u64 = 20_260_901;

#[derive(Parser, Debug)]
#[command(about = "Primitive-function numeric-semantics experiment")]
struct Args {
    #[arg(long, default_value = "experiments/non_polynomial_v1.json")]
    matrix: PathBuf,
    #[arg(long)]
    output_dir: PathBuf,
    #[arg(long, default_value_t = DEFAULT_SEED)]
    seed: u64,
    /// Overrides the matrix for smoke tests without changing the public file.
    #[arg(long)]
    grid_points: Option<usize>,
    #[arg(long)]
    random_samples: Option<usize>,
    #[arg(long)]
    timing_repetitions: Option<usize>,
    #[arg(long)]
    max_configurations: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ExperimentMatrix {
    schema_version: u16,
    matrix_id: String,
    reference_precision_bits: u32,
    validation_precision_bits: u32,
    functions: Vec<FunctionMatrix>,
    formats: Vec<FormatMatrix>,
    roundings: Vec<QuantizationRounding>,
    methods: Vec<MethodMatrix>,
    #[serde(default)]
    explicit_configurations: Vec<ExplicitConfiguration>,
    sampling: SamplingMatrix,
    timing: TimingMatrix,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FunctionMatrix {
    function: FunctionId,
    domain: ValidDomain,
    thresholds: Vec<RationalSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FormatMatrix {
    label: String,
    integer_bits: u16,
    fractional_bits: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MethodMatrix {
    method: ApproximationMethod,
    orders: Vec<u16>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ExplicitConfiguration {
    function: FunctionId,
    format_label: String,
    rounding: QuantizationRounding,
    method: ApproximationMethod,
    order: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SamplingMatrix {
    grid_points: usize,
    random_samples: usize,
    boundary_offset_bits: Vec<u16>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TimingMatrix {
    repetitions: usize,
    batch_size: usize,
}

#[derive(Clone, Debug)]
struct Configuration {
    function: FunctionMatrix,
    format: FormatMatrix,
    rounding: QuantizationRounding,
    method: ApproximationMethod,
    order: u16,
    profile: NumericContractProfile,
}

#[derive(Clone, Debug)]
struct InputSample {
    sample_set: &'static str,
    sample_index: usize,
    value: BigRational,
}

#[derive(Clone, Debug)]
struct EvaluationOutcome {
    input_quantized: BigRational,
    input_quantization_error: BigRational,
    ideal_value: BigRational,
    quantized_coefficients_ideal_value: Option<BigRational>,
    fixed_value: BigRational,
    coefficient_quantization_error: Option<BigRational>,
    operation_quantization_error: Option<BigRational>,
    relation_residual: Option<BigRational>,
    verification_slack_relation: Option<BigRational>,
    verification_output_bound: Option<BigRational>,
    solver_iterations: Option<u32>,
    counts: OperationCounts,
}

#[derive(Clone, Debug, Serialize)]
struct ResultRow {
    schema_version: u16,
    experiment_id: String,
    matrix_id: String,
    sample_set: String,
    sample_index: usize,
    function: String,
    input_exact: String,
    input_f64: f64,
    reference_profile_hash_hex: String,
    numeric_profile_hash_hex: String,
    semantic_class: String,
    representation: String,
    integer_bits: u16,
    fractional_bits: u16,
    rounding: String,
    approximation_method: String,
    approximation_order: u16,
    reference_lower: String,
    reference_nearest: String,
    reference_upper: String,
    reference_binary32_bits: u32,
    reference_auxiliary_f64: f64,
    represented_exact_value: String,
    ideal_approximation_value: String,
    quantized_coefficients_ideal_value: Option<String>,
    computed_value: String,
    input_quantization_error: String,
    representation_error: String,
    approximation_error: String,
    coefficient_quantization_error: Option<String>,
    operation_quantization_error: Option<String>,
    verification_relation_residual: Option<String>,
    verification_slack_relation: Option<String>,
    verification_output_bound: Option<String>,
    absolute_error: String,
    relative_error: String,
    accuracy_bits_absolute: Option<f64>,
    accuracy_bits_relative: Option<f64>,
    total_error_bound: Option<String>,
    total_error_bound_evidence: String,
    decision_threshold: String,
    reference_decision: Option<bool>,
    reference_decision_precision_bits: u32,
    tested_decision: bool,
    predicate_flip: Option<bool>,
    decision_margin: String,
    decision_certification: String,
    decision_certified: bool,
    addition_count: u64,
    multiplication_count: u64,
    division_count: u64,
    inverse_count: u64,
    term_count: u64,
    operation_count: u64,
    numerical_multiplicative_depth: u32,
    verification_relation_depth: Option<u32>,
    conceptual_witness_bytes: u64,
    solver_iterations: Option<u32>,
    evaluation_runtime_ns: u128,
    estimated_constraints: Option<u64>,
    actual_constraints: Option<u64>,
}

#[derive(Clone, Debug, Default)]
struct SummaryAccumulator {
    reference_inconclusive: u64,
    function: String,
    method: String,
    order: u16,
    integer_bits: u16,
    fractional_bits: u16,
    rounding: String,
    semantic_class: String,
    numeric_samples: u64,
    sum_squared_error: f64,
    max_absolute_error: f64,
    decision_opportunities: u64,
    predicate_flips: u64,
    first_predicate_flip: Option<FirstPredicateFlip>,
    certified_stable: u64,
    ambiguous: u64,
    observed_same: u64,
    observed_flip: u64,
    evaluation_runtime_ns: u128,
    counts: OperationCounts,
}

#[derive(Clone, Debug, Serialize)]
struct FirstPredicateFlip {
    sample_set: String,
    sample_index: usize,
    decision_threshold: String,
}

#[derive(Clone, Debug, Serialize)]
struct SummaryRecord {
    reference_inconclusive_count: u64,
    numeric_profile_hash_hex: String,
    function: String,
    approximation_method: String,
    approximation_order: u16,
    integer_bits: u16,
    fractional_bits: u16,
    rounding: String,
    semantic_class: String,
    numeric_samples: u64,
    max_absolute_error: f64,
    rmse: f64,
    decision_opportunities: u64,
    predicate_flip_count: u64,
    first_predicate_flip: Option<FirstPredicateFlip>,
    certified_stable_count: u64,
    ambiguous_count: u64,
    observed_same_count: u64,
    observed_flip_count: u64,
    certified_coverage: f64,
    mean_evaluation_runtime_ns: f64,
    operation_count_per_evaluation: u64,
    multiplication_count_per_evaluation: u64,
    numerical_multiplicative_depth: u32,
    verification_relation_depth: Option<u32>,
    estimated_constraints: Option<u64>,
    actual_constraints: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
struct TimingRecord {
    numeric_profile_hash_hex: String,
    repetitions: usize,
    batch_size: usize,
    median_batch_ns: u128,
    p05_batch_ns: u128,
    p95_batch_ns: u128,
}

#[derive(Clone, Debug, Serialize)]
struct Manifest {
    schema_version: u16,
    matrix_id: String,
    seed: u64,
    matrix_path: String,
    matrix_sha256: String,
    effective_matrix_sha256: String,
    max_configurations: Option<usize>,
    source_file_sha256: BTreeMap<String, String>,
    input_set_sha256: String,
    cargo_lock_sha256: String,
    git_commit: String,
    git_dirty: bool,
    git_diff_sha256: String,
    rustc_version: String,
    operating_system: String,
    architecture: String,
    process_peak_rss_bytes: Option<u64>,
    reference_precision_bits: u32,
    validation_precision_bits: u32,
    numeric_profile_hashes: Vec<String>,
    source_paper: String,
    source_artifact_doi: String,
    estimated_constraints_available: bool,
    actual_constraints_available: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.output_dir.exists() {
        bail!(
            "output-dir já existe; resultados nunca são sobrescritos: {}",
            args.output_dir.display()
        );
    }
    let matrix_bytes = fs::read(&args.matrix)
        .with_context(|| format!("falha ao ler {}", args.matrix.display()))?;
    let mut matrix: ExperimentMatrix =
        serde_json::from_slice(&matrix_bytes).context("matriz experimental JSON inválida")?;
    validate_matrix(&matrix)?;
    if let Some(value) = args.grid_points {
        matrix.sampling.grid_points = value;
    }
    if let Some(value) = args.random_samples {
        matrix.sampling.random_samples = value;
    }
    if let Some(value) = args.timing_repetitions {
        matrix.timing.repetitions = value;
    }
    validate_matrix(&matrix)?;

    let mut configurations = materialize_configurations(&matrix)?;
    if let Some(limit) = args.max_configurations {
        configurations.truncate(limit);
    }
    if configurations.is_empty() {
        bail!("matriz não produziu configurações válidas");
    }
    let inputs_by_function = build_all_inputs(&matrix, args.seed)?;
    let prepared_gl = prepare_gl_rules(&configurations)?;

    fs::create_dir_all(args.output_dir.join("profiles"))?;
    write_json(args.output_dir.join("effective_matrix.json"), &matrix)?;
    let raw_file = File::create(args.output_dir.join("raw.jsonl"))?;
    let mut raw_json = BufWriter::new(raw_file);
    let mut raw_csv = Writer::from_path(args.output_dir.join("raw.csv"))?;
    let mut summaries: BTreeMap<String, SummaryAccumulator> = BTreeMap::new();
    let mut profile_hashes = BTreeSet::new();
    let mut reference_profiles = HashMap::new();

    for function in &matrix.functions {
        let profile = reference_profile(function, matrix.reference_precision_bits)?;
        let hash = profile.profile_hash_hex();
        write_profile_once(&args.output_dir, &profile)?;
        profile_hashes.insert(hash.clone());
        reference_profiles.insert(function.function.tag().to_string(), (profile, hash));
    }

    for configuration in &configurations {
        let profile_hash = configuration.profile.profile_hash_hex();
        write_profile_once(&args.output_dir, &configuration.profile)?;
        profile_hashes.insert(profile_hash.clone());
        let (_, reference_hash) = reference_profiles
            .get(configuration.function.function.tag())
            .expect("referência materializada");
        let inputs = inputs_by_function
            .get(configuration.function.function.tag())
            .expect("inputs materializados");
        let format = fixed_format(configuration);
        let thresholds = parse_thresholds(&configuration.function.thresholds)?;
        let summary = summaries
            .entry(profile_hash.clone())
            .or_insert_with(|| new_summary(configuration));

        for sample in inputs {
            let reference = evaluate_transcendental(
                configuration.function.function,
                &sample.value,
                matrix.reference_precision_bits,
            )?;
            let validation = evaluate_transcendental(
                configuration.function.function,
                &sample.value,
                matrix.validation_precision_bits,
            )?;
            if !reference.contains(&validation.nearest_rational()) {
                bail!("referência de validação escapou do enclosure principal");
            }
            let started = Instant::now();
            let outcome =
                evaluate_configuration(configuration, &sample.value, &format, &prepared_gl)?;
            let elapsed = started.elapsed().as_nanos();
            let quantized_reference = evaluate_transcendental(
                configuration.function.function,
                &outcome.input_quantized,
                matrix.reference_precision_bits,
            )?;
            let represented_exact = format
                .encode(&quantized_reference.nearest_rational())?
                .value
                .decode();
            let reference_value = reference.nearest_rational();
            let quantized_input_reference = quantized_reference.nearest_rational();
            let absolute_error = (&outcome.fixed_value - &reference_value).abs();
            let relative_error = if reference_value.is_zero() {
                BigRational::zero()
            } else {
                &absolute_error / reference_value.abs()
            };
            let representation_error = (&represented_exact - &reference_value).abs();
            let approximation_error = (&outcome.ideal_value - &quantized_input_reference).abs();
            let total_error_bound = analytical_total_bound(configuration, &format, &outcome);

            summary.numeric_samples += 1;
            let absolute_f64 = rational_to_f64(&absolute_error);
            summary.sum_squared_error += absolute_f64 * absolute_f64;
            summary.max_absolute_error = summary.max_absolute_error.max(absolute_f64);
            summary.evaluation_runtime_ns += elapsed;
            summary.counts = outcome.counts.clone();

            for threshold in &thresholds {
                let (reference_decision, reference_decision_precision_bits) =
                    if let Some(decision) = reference.ge_threshold(threshold) {
                        (Some(decision), matrix.reference_precision_bits)
                    } else {
                        resolve_transcendental_decision(
                            configuration.function.function,
                            &sample.value,
                            threshold,
                            matrix.validation_precision_bits,
                            4096,
                        )?
                    };
                let tested_decision = outcome.fixed_value >= *threshold;
                let predicate_flip = reference_decision.map(|value| value != tested_decision);
                if predicate_flip == Some(true) && summary.first_predicate_flip.is_none() {
                    summary.first_predicate_flip = Some(FirstPredicateFlip {
                        sample_set: sample.sample_set.into(),
                        sample_index: sample.sample_index,
                        decision_threshold: rational_string(threshold),
                    });
                }
                let decision_margin = (&outcome.fixed_value - threshold).abs();
                let certification = if let Some(reference_decision) = reference_decision {
                    classify_decision(
                        &outcome.fixed_value,
                        threshold,
                        total_error_bound.as_ref(),
                        reference_decision,
                    )?
                } else {
                    summary.reference_inconclusive += 1;
                    DecisionCertification::Ambiguous
                };
                update_decision_summary(summary, certification, predicate_flip == Some(true));
                let experiment_id = format!(
                    "{}-{}-{}-{:06}",
                    &profile_hash[..12],
                    sample.sample_set,
                    sample.sample_index,
                    summary.decision_opportunities
                );
                let row = ResultRow {
                    schema_version: RESULT_SCHEMA_VERSION,
                    experiment_id,
                    matrix_id: matrix.matrix_id.clone(),
                    sample_set: sample.sample_set.into(),
                    sample_index: sample.sample_index,
                    function: configuration.function.function.tag().into(),
                    input_exact: rational_string(&sample.value),
                    input_f64: rational_to_f64(&sample.value),
                    reference_profile_hash_hex: reference_hash.clone(),
                    numeric_profile_hash_hex: profile_hash.clone(),
                    semantic_class: semantic_class_tag(configuration.profile.semantic_class())
                        .into(),
                    representation: "signed-fixed-point".into(),
                    integer_bits: configuration.format.integer_bits,
                    fractional_bits: configuration.format.fractional_bits,
                    rounding: rounding_tag(configuration.rounding).into(),
                    approximation_method: configuration.method.tag().into(),
                    approximation_order: configuration.order,
                    reference_lower: rational_string(&reference.lower_rational()),
                    reference_nearest: rational_string(&reference_value),
                    reference_upper: rational_string(&reference.upper_rational()),
                    reference_binary32_bits: reference.binary32.to_bits(),
                    reference_auxiliary_f64: reference.auxiliary_f64,
                    represented_exact_value: rational_string(&represented_exact),
                    ideal_approximation_value: rational_string(&outcome.ideal_value),
                    quantized_coefficients_ideal_value: outcome
                        .quantized_coefficients_ideal_value
                        .as_ref()
                        .map(rational_string),
                    computed_value: rational_string(&outcome.fixed_value),
                    input_quantization_error: rational_string(&outcome.input_quantization_error),
                    representation_error: rational_string(&representation_error),
                    approximation_error: rational_string(&approximation_error),
                    coefficient_quantization_error: outcome
                        .coefficient_quantization_error
                        .as_ref()
                        .map(rational_string),
                    operation_quantization_error: outcome
                        .operation_quantization_error
                        .as_ref()
                        .map(rational_string),
                    verification_relation_residual: outcome
                        .relation_residual
                        .as_ref()
                        .map(rational_string),
                    verification_slack_relation: outcome
                        .verification_slack_relation
                        .as_ref()
                        .map(rational_string),
                    verification_output_bound: outcome
                        .verification_output_bound
                        .as_ref()
                        .map(rational_string),
                    absolute_error: rational_string(&absolute_error),
                    relative_error: rational_string(&relative_error),
                    accuracy_bits_absolute: accuracy_bits(&absolute_error),
                    accuracy_bits_relative: accuracy_bits(&relative_error),
                    total_error_bound: total_error_bound.as_ref().map(rational_string),
                    total_error_bound_evidence: if total_error_bound.is_some() {
                        "analytical"
                    } else {
                        "observed-grid"
                    }
                    .into(),
                    decision_threshold: rational_string(threshold),
                    reference_decision,
                    reference_decision_precision_bits,
                    tested_decision,
                    predicate_flip,
                    decision_margin: rational_string(&decision_margin),
                    decision_certification: if reference_decision.is_none() {
                        "reference-inconclusive"
                    } else {
                        decision_certification_tag(certification)
                    }
                    .into(),
                    decision_certified: certification == DecisionCertification::CertifiedStable,
                    addition_count: outcome.counts.additions,
                    multiplication_count: outcome.counts.multiplications,
                    division_count: outcome.counts.divisions,
                    inverse_count: outcome.counts.inversions,
                    term_count: outcome.counts.terms,
                    operation_count: outcome.counts.total_operations(),
                    numerical_multiplicative_depth: outcome.counts.numerical_multiplicative_depth,
                    verification_relation_depth: outcome.counts.verification_relation_depth,
                    conceptual_witness_bytes: outcome.counts.conceptual_witness_bytes,
                    solver_iterations: outcome.solver_iterations,
                    evaluation_runtime_ns: elapsed,
                    estimated_constraints: None,
                    actual_constraints: None,
                };
                serde_json::to_writer(&mut raw_json, &row)?;
                raw_json.write_all(b"\n")?;
                raw_csv.serialize(&row)?;
            }
        }
    }
    raw_json.flush()?;
    raw_csv.flush()?;

    let summary_records = finalize_summaries(summaries);
    write_json(args.output_dir.join("summary.json"), &summary_records)?;
    let timing = run_timing(
        &configurations,
        &inputs_by_function,
        &prepared_gl,
        matrix.timing.repetitions,
        matrix.timing.batch_size,
    )?;
    write_json(args.output_dir.join("timing.json"), &timing)?;

    let manifest = build_manifest(
        &matrix,
        &args,
        &matrix_bytes,
        &inputs_by_function,
        profile_hashes.into_iter().collect(),
    )?;
    write_json(args.output_dir.join("manifest.json"), &manifest)?;
    Ok(())
}

fn validate_matrix(matrix: &ExperimentMatrix) -> Result<()> {
    if matrix.schema_version != 1 || matrix.matrix_id.trim().is_empty() {
        bail!("schema ou matrix_id inválido");
    }
    if matrix.reference_precision_bits < 128
        || matrix.validation_precision_bits <= matrix.reference_precision_bits
        || matrix.validation_precision_bits > 4096
    {
        bail!("precisões MPFR devem satisfazer validation > reference >= 128");
    }
    if matrix.functions.is_empty()
        || matrix.formats.is_empty()
        || matrix.roundings.is_empty()
        || matrix.methods.is_empty()
    {
        bail!("matriz deve declarar funções, formatos, arredondamentos e métodos");
    }
    if matrix.sampling.grid_points < 2 || matrix.timing.batch_size == 0 {
        bail!("amostragem ou timing inválido");
    }
    for function in &matrix.functions {
        let (minimum, maximum) = function.domain.bounds()?;
        if minimum >= maximum || function.thresholds.is_empty() {
            bail!(
                "domínio ou thresholds inválidos para {:?}",
                function.function
            );
        }
        if !matches!(function.function, FunctionId::Exp | FunctionId::Sigmoid)
            || minimum < BigRational::from_integer((-1).into())
            || maximum > BigRational::one()
        {
            bail!("o protocolo v2 exige exp/sigmoid e domínio contido em [-1,1]");
        }
    }
    for method in &matrix.methods {
        if method.method == ApproximationMethod::None
            || method.orders.is_empty()
            || method.orders.contains(&0)
        {
            bail!("método experimental inválido");
        }
    }
    Ok(())
}

fn materialize_configurations(matrix: &ExperimentMatrix) -> Result<Vec<Configuration>> {
    if !matrix.explicit_configurations.is_empty() {
        let mut configurations = Vec::with_capacity(matrix.explicit_configurations.len());
        for explicit in &matrix.explicit_configurations {
            let function = matrix
                .functions
                .iter()
                .find(|candidate| candidate.function == explicit.function)
                .with_context(|| format!("função explícita ausente: {:?}", explicit.function))?;
            let format = matrix
                .formats
                .iter()
                .find(|candidate| candidate.label == explicit.format_label)
                .with_context(|| format!("formato explícito ausente: {}", explicit.format_label))?;
            if explicit.method == ApproximationMethod::None || explicit.order == 0 {
                bail!("configuração explícita possui método/ordem inválidos");
            }
            configurations.push(Configuration {
                function: function.clone(),
                format: format.clone(),
                rounding: explicit.rounding,
                method: explicit.method,
                order: explicit.order,
                profile: materialize_profile(
                    function,
                    format,
                    explicit.rounding,
                    explicit.method,
                    explicit.order,
                )?,
            });
        }
        return Ok(configurations);
    }
    let mut configurations = Vec::new();
    for function in &matrix.functions {
        for format in &matrix.formats {
            for &rounding in &matrix.roundings {
                for method in &matrix.methods {
                    for &order in &method.orders {
                        let profile =
                            materialize_profile(function, format, rounding, method.method, order)?;
                        configurations.push(Configuration {
                            function: function.clone(),
                            format: format.clone(),
                            rounding,
                            method: method.method,
                            order,
                            profile,
                        });
                    }
                }
            }
        }
    }
    Ok(configurations)
}

fn materialize_profile(
    function: &FunctionMatrix,
    format: &FormatMatrix,
    rounding: QuantizationRounding,
    method: ApproximationMethod,
    order: u16,
) -> Result<NumericContractProfile> {
    let representation = RepresentationSpec {
        kind: NumericRepresentationKind::SignedFixedPoint,
        signed: true,
        integer_bits: format.integer_bits,
        fractional_bits: format.fractional_bits,
        scale_power: format.fractional_bits,
    };
    let operations = OperationSemantics {
        rounding,
        rescaling: RescalingMode::AfterEveryMultiplication,
        division_rounding: rounding,
        overflow: OverflowMode::Reject,
        underflow: UnderflowMode::QuantizeToGrid,
        exceptional_values: ExceptionalValuePolicy::RejectNonFinite,
    };
    let source = if method == ApproximationMethod::Taylor {
        SourceKind::RepositoryDerived
    } else {
        SourceKind::PaperDerived
    };
    let approximation = ApproximationSpec {
        function: function.function,
        method,
        method_version: 2,
        order,
        expansion_center: RationalSpec::integer(0),
        evaluation_form: match method {
            ApproximationMethod::Taylor => "horner-rounded-after-each-multiplication",
            ApproximationMethod::PadeDiagonal => "diagonal-rational-horner-cross-multiplication",
            ApproximationMethod::GaussLegendreImplicit => {
                "stable-implicit-inverse-derivative-relation"
            }
            ApproximationMethod::None => unreachable!(),
        }
        .into(),
        parameters: BTreeMap::new(),
        coefficient_or_node_source: match method {
            ApproximationMethod::Taylor => "exact-rational-series",
            ApproximationMethod::PadeDiagonal => "exact-rational-series-derived-pade",
            ApproximationMethod::GaussLegendreImplicit => "mpfr-512-legendre-roots-weights",
            ApproximationMethod::None => unreachable!(),
        }
        .into(),
    };
    let quantization = ErrorComponent {
        rule: named_rule("fixed-grid-rounding-bound"),
        evidence: EvidenceLevel::Analytical,
        provenance: SourceKind::RepositoryDerived,
        citation: "Exact grid bound from the declared rounding rule".into(),
    };
    let approximation_error =
        if method == ApproximationMethod::Taylor && function.function == FunctionId::Exp {
            ErrorComponent {
                rule: named_rule("exp-taylor-lagrange-remainder-on-minus-one-one"),
                evidence: EvidenceLevel::Analytical,
                provenance: SourceKind::RepositoryDerived,
                citation: "Lagrange remainder with exp(x) <= 3 on [-1,1]".into(),
            }
        } else {
            observed_component("approximation-error-measured-against-directed-mpfr")
        };
    let verification_slack = if method == ApproximationMethod::GaussLegendreImplicit {
        ErrorComponent {
            rule: named_rule("sridhar-gl-four-bit-relation-slack"),
            evidence: EvidenceLevel::ModelAssumption,
            provenance: SourceKind::OurExtension,
            citation: "Historical 16-ULP model inspired by Sridhar section 4.5.1; not a validated universal bound for this evaluator".into(),
        }
    } else {
        ErrorComponent::exact_zero(
            SourceKind::RepositoryDerived,
            "off-circuit deterministic evaluation",
        )
    };
    let accumulated = if method == ApproximationMethod::Taylor
        && function.function == FunctionId::Exp
    {
        ErrorComponent {
            rule: named_rule("exp-taylor-fixed-horner-conservative-bound-v1"),
            evidence: EvidenceLevel::Analytical,
            provenance: SourceKind::OurExtension,
            citation: "Repository derivation documented in numeric semantics methodology".into(),
        }
    } else {
        observed_component("end-to-end-error-measured-against-directed-mpfr")
    };
    let error_contract = ErrorContract {
        quantization,
        approximation: approximation_error,
        verification_slack,
        accumulated_error: accumulated,
        composition_rule: match method {
            ApproximationMethod::GaussLegendreImplicit => {
                CompositionRule::ImplicitRelationMonotoneEnclosure
            }
            _ => CompositionRule::ConservativeSum,
        },
    };
    let rounding_profile = match rounding {
        QuantizationRounding::Floor => "sridhar-paper-floor",
        QuantizationRounding::NearestTiesToEven => "sridhar-artifact-rne",
        QuantizationRounding::TowardZero => "repository-toward-zero",
    };
    let id = format!(
        "{}-{}-{}-{}-{}-audit-v2",
        format.label,
        function.function.tag(),
        method.tag(),
        order,
        rounding_profile
    );
    NumericContractProfileBuilder::new(
        id,
        representation,
        operations,
        function.domain.clone(),
        approximation,
        error_contract,
    )
    .provenance(ProfileProvenance {
        source_kind: source,
        citation: if source == SourceKind::PaperDerived {
            "Efficiently Provable Approximations for Non-Polynomial Functions".into()
        } else {
            "proof-numeric-zk baseline".into()
        },
        locator: method.tag().into(),
        note: "Formula provenance; decision analysis is an extension of this repository".into(),
    })
    .provenance(ProfileProvenance {
        source_kind: match rounding {
            QuantizationRounding::Floor => SourceKind::PaperDerived,
            QuantizationRounding::NearestTiesToEven => SourceKind::ArtifactDerived,
            QuantizationRounding::TowardZero => SourceKind::RepositoryDerived,
        },
        citation: match rounding {
            QuantizationRounding::Floor => "Sridhar paper fixed-point encoding floor(a*2^s)",
            QuantizationRounding::NearestTiesToEven => {
                "Official artifact mpmath 1.3.0 nint quantization"
            }
            QuantizationRounding::TowardZero => "proof-numeric-zk truncation baseline",
        }
        .into(),
        locator: "rounding".into(),
        note: "Paper and artifact rounding are intentionally separate profiles".into(),
    })
    .build()
    .map_err(Into::into)
}

fn reference_profile(
    function: &FunctionMatrix,
    precision_bits: u32,
) -> Result<NumericContractProfile> {
    let zero = ErrorComponent::exact_zero(
        SourceKind::RepositoryDerived,
        "declared MPFR reference semantics",
    );
    NumericContractProfileBuilder::new(
        format!(
            "mpfr-{precision_bits}-{}-reference",
            function.function.tag()
        ),
        RepresentationSpec {
            kind: NumericRepresentationKind::MpfrReference,
            signed: true,
            integer_bits: 16,
            fractional_bits: precision_bits as u16,
            scale_power: precision_bits as u16,
        },
        OperationSemantics {
            rounding: QuantizationRounding::NearestTiesToEven,
            rescaling: RescalingMode::NotApplicable,
            division_rounding: QuantizationRounding::NearestTiesToEven,
            overflow: OverflowMode::Reject,
            underflow: UnderflowMode::QuantizeToGrid,
            exceptional_values: ExceptionalValuePolicy::RejectNonFinite,
        },
        function.domain.clone(),
        ApproximationSpec {
            function: function.function,
            method: ApproximationMethod::None,
            method_version: 1,
            order: 0,
            expansion_center: RationalSpec::integer(0),
            evaluation_form: "mpfr-directed-rounding-enclosure".into(),
            parameters: BTreeMap::new(),
            coefficient_or_node_source: "mpfr".into(),
        },
        ErrorContract {
            quantization: zero.clone(),
            approximation: zero.clone(),
            verification_slack: zero.clone(),
            accumulated_error: zero,
            composition_rule: CompositionRule::IntervalPropagation,
        },
    )
    .provenance(ProfileProvenance {
        source_kind: SourceKind::RepositoryDerived,
        citation: "MPFR via rug with directed rounding".into(),
        locator: "numeric-reference::transcendental".into(),
        note: "Exact reference semantics are relative to this declared MPFR profile".into(),
    })
    .build()
    .map_err(Into::into)
}

fn named_rule(name: &str) -> BoundRule {
    BoundRule::Named {
        rule_id: name.into(),
        parameters: BTreeMap::new(),
    }
}

fn observed_component(rule_id: &str) -> ErrorComponent {
    ErrorComponent {
        rule: named_rule(rule_id),
        evidence: EvidenceLevel::ObservedGrid,
        provenance: SourceKind::RepositoryDerived,
        citation: "Measured by the function-eval harness; not a global guarantee".into(),
    }
}

fn fixed_format(configuration: &Configuration) -> FixedPointFormat {
    FixedPointFormat {
        integer_bits: configuration.format.integer_bits,
        fractional_bits: configuration.format.fractional_bits,
        signed: true,
        rounding: configuration.rounding,
        overflow: OverflowMode::Reject,
    }
}

fn prepare_gl_rules(
    configurations: &[Configuration],
) -> Result<HashMap<(String, u16), PreparedGaussLegendre>> {
    let mut prepared = HashMap::new();
    for configuration in configurations {
        if configuration.method != ApproximationMethod::GaussLegendreImplicit {
            continue;
        }
        let key = (
            configuration.function.function.tag().to_string(),
            configuration.order,
        );
        if let std::collections::hash_map::Entry::Vacant(entry) = prepared.entry(key) {
            entry.insert(prepare_gauss_legendre(
                configuration.function.function,
                usize::from(configuration.order),
            )?);
        }
    }
    Ok(prepared)
}

fn evaluate_configuration(
    configuration: &Configuration,
    input: &BigRational,
    format: &FixedPointFormat,
    prepared_gl: &HashMap<(String, u16), PreparedGaussLegendre>,
) -> Result<EvaluationOutcome> {
    match configuration.method {
        ApproximationMethod::Taylor => polynomial_outcome(evaluate_taylor(
            configuration.function.function,
            usize::from(configuration.order),
            input,
            format,
        )?),
        ApproximationMethod::PadeDiagonal => polynomial_outcome(evaluate_pade(
            configuration.function.function,
            usize::from(configuration.order),
            input,
            format,
        )?),
        ApproximationMethod::GaussLegendreImplicit => {
            let key = (
                configuration.function.function.tag().to_string(),
                configuration.order,
            );
            let prepared = prepared_gl.get(&key).expect("GL preparado");
            gl_outcome(evaluate_gauss_legendre_prepared(prepared, input, format)?)
        }
        ApproximationMethod::None => bail!("método none não é executável"),
    }
}

fn polynomial_outcome(value: PolynomialEvaluation) -> Result<EvaluationOutcome> {
    let mut counts = value.counts;
    counts.verification_relation_depth = None;
    Ok(EvaluationOutcome {
        input_quantized: value.input_quantized,
        input_quantization_error: value.input_quantization_error,
        ideal_value: value.ideal_value,
        quantized_coefficients_ideal_value: Some(value.quantized_coefficients_ideal_value),
        fixed_value: value.fixed_value,
        coefficient_quantization_error: Some(value.coefficient_quantization_error_sum),
        operation_quantization_error: Some(value.operation_quantization_error),
        relation_residual: None,
        verification_slack_relation: Some(BigRational::zero()),
        verification_output_bound: Some(BigRational::zero()),
        solver_iterations: None,
        counts,
    })
}

fn gl_outcome(value: GaussLegendreEvaluation) -> Result<EvaluationOutcome> {
    let mut counts = value.counts;
    counts.verification_relation_depth = None;
    Ok(EvaluationOutcome {
        input_quantized: value.input_quantized,
        input_quantization_error: value.input_quantization_error,
        ideal_value: value.ideal_value,
        quantized_coefficients_ideal_value: None,
        fixed_value: value.fixed_value,
        coefficient_quantization_error: None,
        operation_quantization_error: None,
        relation_residual: Some(value.relation_residual),
        verification_slack_relation: Some(value.paper_relation_slack_bound),
        verification_output_bound: None,
        solver_iterations: Some(value.solver_iterations),
        counts,
    })
}

fn analytical_total_bound(
    configuration: &Configuration,
    format: &FixedPointFormat,
    outcome: &EvaluationOutcome,
) -> Option<BigRational> {
    if configuration.function.function != FunctionId::Exp
        || configuration.method != ApproximationMethod::Taylor
    {
        return None;
    }
    let degree = usize::from(configuration.order);
    let q = format.absolute_quantization_bound();
    let x = outcome.input_quantized.abs();
    let factorial = (1..=degree + 1).fold(BigInt::one(), |accumulator, value| accumulator * value);
    let approximation = BigRational::from_integer(3.into()) * rational_pow(&x, degree + 1)
        / BigRational::from_integer(factorial);
    let input_effect = BigRational::from_integer(3.into()) * &q;
    let mut horner = q.clone();
    let growth = BigRational::one() + &q;
    for _ in 0..degree {
        horner = &growth * horner + BigRational::from_integer(3.into()) * &q + &q + &q;
    }
    Some(approximation + input_effect + horner)
}

fn classify_decision(
    value: &BigRational,
    threshold: &BigRational,
    total_bound: Option<&BigRational>,
    reference_decision: bool,
) -> Result<DecisionCertification> {
    let tested = value >= threshold;
    let Some(bound) = total_bound else {
        return Ok(if tested == reference_decision {
            DecisionCertification::ObservedSame
        } else {
            DecisionCertification::ObservedFlip
        });
    };
    let lower = value - bound;
    let upper = value + bound;
    let certified = if lower >= *threshold {
        Some(true)
    } else if upper < *threshold {
        Some(false)
    } else {
        None
    };
    match certified {
        None => Ok(DecisionCertification::Ambiguous),
        Some(decision) => {
            if decision != reference_decision {
                bail!("decisão certificada contradiz a referência dirigida");
            }
            Ok(DecisionCertification::CertifiedStable)
        }
    }
}

fn build_all_inputs(
    matrix: &ExperimentMatrix,
    seed: u64,
) -> Result<HashMap<String, Vec<InputSample>>> {
    let mut all = HashMap::new();
    for (function_index, function) in matrix.functions.iter().enumerate() {
        let mut inputs = Vec::new();
        let (minimum, maximum) = function.domain.bounds()?;
        let denominator =
            BigRational::from_integer(BigInt::from(matrix.sampling.grid_points.saturating_sub(1)));
        for index in 0..matrix.sampling.grid_points {
            let ratio = BigRational::from_integer(BigInt::from(index)) / &denominator;
            inputs.push(InputSample {
                sample_set: "grid",
                sample_index: index,
                value: &minimum + (&maximum - &minimum) * ratio,
            });
        }
        let mut generator = SplitMix64::new(seed ^ ((function_index as u64 + 1) << 32));
        let random_denominator = BigRational::from_integer(BigInt::one() << 64_usize);
        for index in 0..matrix.sampling.random_samples {
            let unit =
                BigRational::from_integer(BigInt::from(generator.next())) / &random_denominator;
            inputs.push(InputSample {
                sample_set: "random",
                sample_index: index,
                value: &minimum + (&maximum - &minimum) * unit,
            });
        }
        let mut boundary_index = 0;
        for threshold in parse_thresholds(&function.thresholds)? {
            if let Some(center) = inverse_threshold(function.function, &threshold) {
                for offset_bits in &matrix.sampling.boundary_offset_bits {
                    let offset =
                        BigRational::new(BigInt::one(), BigInt::one() << usize::from(*offset_bits));
                    for sign in [-1_i32, 0, 1] {
                        let candidate = match sign {
                            -1 => &center - &offset,
                            0 => center.clone(),
                            1 => &center + &offset,
                            _ => unreachable!(),
                        };
                        if candidate >= minimum && candidate <= maximum {
                            inputs.push(InputSample {
                                sample_set: "boundary",
                                sample_index: boundary_index,
                                value: candidate,
                            });
                            boundary_index += 1;
                        }
                    }
                }
            }
        }
        let mut seen = BTreeSet::new();
        inputs.retain(|sample| seen.insert(rational_string(&sample.value)));
        all.insert(function.function.tag().to_string(), inputs);
    }
    Ok(all)
}

fn inverse_threshold(function: FunctionId, threshold: &BigRational) -> Option<BigRational> {
    let precision = 512;
    match function {
        FunctionId::Exp => {
            if threshold <= &BigRational::zero() {
                return None;
            }
            let value = rational_to_float(threshold, precision).ln();
            Some(float_to_big_rational(&value))
        }
        FunctionId::Sigmoid => {
            if threshold <= &BigRational::zero() || threshold >= &BigRational::one() {
                return None;
            }
            let ratio = threshold / (BigRational::one() - threshold);
            let value = rational_to_float(&ratio, precision).ln();
            Some(float_to_big_rational(&value))
        }
        _ => None,
    }
}

fn parse_thresholds(values: &[RationalSpec]) -> Result<Vec<BigRational>> {
    values
        .iter()
        .map(|value| value.to_rational().map_err(Into::into))
        .collect()
}

fn run_timing(
    configurations: &[Configuration],
    inputs_by_function: &HashMap<String, Vec<InputSample>>,
    prepared_gl: &HashMap<(String, u16), PreparedGaussLegendre>,
    repetitions: usize,
    batch_size: usize,
) -> Result<Vec<TimingRecord>> {
    let mut samples: Vec<Vec<u128>> = vec![Vec::with_capacity(repetitions); configurations.len()];
    for configuration in configurations {
        let inputs = timing_inputs(
            inputs_by_function
                .get(configuration.function.function.tag())
                .expect("inputs de timing"),
            batch_size,
        );
        let format = fixed_format(configuration);
        for input in &inputs {
            let _ = evaluate_configuration(configuration, input, &format, prepared_gl)?;
        }
    }
    for repetition in 0..repetitions {
        for offset in 0..configurations.len() {
            let index = (offset + repetition) % configurations.len();
            let configuration = &configurations[index];
            let inputs = timing_inputs(
                inputs_by_function
                    .get(configuration.function.function.tag())
                    .expect("inputs de timing"),
                batch_size,
            );
            let format = fixed_format(configuration);
            let started = Instant::now();
            for input in &inputs {
                let _ = evaluate_configuration(configuration, input, &format, prepared_gl)?;
            }
            samples[index].push(started.elapsed().as_nanos());
        }
    }
    let mut records = Vec::new();
    for (configuration, mut values) in configurations.iter().zip(samples) {
        values.sort_unstable();
        let median = percentile(&values, 0.50);
        let p05 = percentile(&values, 0.05);
        let p95 = percentile(&values, 0.95);
        records.push(TimingRecord {
            numeric_profile_hash_hex: configuration.profile.profile_hash_hex(),
            repetitions,
            batch_size: timing_inputs(
                inputs_by_function
                    .get(configuration.function.function.tag())
                    .expect("inputs de timing"),
                batch_size,
            )
            .len(),
            median_batch_ns: median,
            p05_batch_ns: p05,
            p95_batch_ns: p95,
        });
    }
    Ok(records)
}

fn timing_inputs(samples: &[InputSample], batch_size: usize) -> Vec<BigRational> {
    let mut selected: Vec<_> = samples
        .iter()
        .filter(|sample| sample.sample_set == "random")
        .take(batch_size)
        .map(|sample| sample.value.clone())
        .collect();
    if selected.len() < batch_size {
        selected.extend(
            samples
                .iter()
                .take(batch_size - selected.len())
                .map(|sample| sample.value.clone()),
        );
    }
    selected
}

fn percentile(values: &[u128], fraction: f64) -> u128 {
    if values.is_empty() {
        return 0;
    }
    let index = ((values.len() - 1) as f64 * fraction).round() as usize;
    values[index]
}

fn new_summary(configuration: &Configuration) -> SummaryAccumulator {
    SummaryAccumulator {
        function: configuration.function.function.tag().into(),
        method: configuration.method.tag().into(),
        order: configuration.order,
        integer_bits: configuration.format.integer_bits,
        fractional_bits: configuration.format.fractional_bits,
        rounding: rounding_tag(configuration.rounding).into(),
        semantic_class: semantic_class_tag(configuration.profile.semantic_class()).into(),
        ..SummaryAccumulator::default()
    }
}

fn update_decision_summary(
    summary: &mut SummaryAccumulator,
    certification: DecisionCertification,
    predicate_flip: bool,
) {
    summary.decision_opportunities += 1;
    summary.predicate_flips += u64::from(predicate_flip);
    match certification {
        DecisionCertification::CertifiedStable => summary.certified_stable += 1,
        DecisionCertification::Ambiguous => summary.ambiguous += 1,
        DecisionCertification::ObservedSame => summary.observed_same += 1,
        DecisionCertification::ObservedFlip => summary.observed_flip += 1,
    }
}

fn finalize_summaries(summaries: BTreeMap<String, SummaryAccumulator>) -> Vec<SummaryRecord> {
    summaries
        .into_iter()
        .map(|(hash, summary)| SummaryRecord {
            reference_inconclusive_count: summary.reference_inconclusive,
            numeric_profile_hash_hex: hash,
            function: summary.function,
            approximation_method: summary.method,
            approximation_order: summary.order,
            integer_bits: summary.integer_bits,
            fractional_bits: summary.fractional_bits,
            rounding: summary.rounding,
            semantic_class: summary.semantic_class,
            numeric_samples: summary.numeric_samples,
            max_absolute_error: summary.max_absolute_error,
            rmse: if summary.numeric_samples == 0 {
                0.0
            } else {
                (summary.sum_squared_error / summary.numeric_samples as f64).sqrt()
            },
            decision_opportunities: summary.decision_opportunities,
            predicate_flip_count: summary.predicate_flips,
            first_predicate_flip: summary.first_predicate_flip,
            certified_stable_count: summary.certified_stable,
            ambiguous_count: summary.ambiguous,
            observed_same_count: summary.observed_same,
            observed_flip_count: summary.observed_flip,
            certified_coverage: if summary.decision_opportunities == 0 {
                0.0
            } else {
                summary.certified_stable as f64 / summary.decision_opportunities as f64
            },
            mean_evaluation_runtime_ns: if summary.numeric_samples == 0 {
                0.0
            } else {
                summary.evaluation_runtime_ns as f64 / summary.numeric_samples as f64
            },
            operation_count_per_evaluation: summary.counts.total_operations(),
            multiplication_count_per_evaluation: summary.counts.multiplications,
            numerical_multiplicative_depth: summary.counts.numerical_multiplicative_depth,
            verification_relation_depth: summary.counts.verification_relation_depth,
            estimated_constraints: None,
            actual_constraints: None,
        })
        .collect()
}

fn write_profile_once(output_dir: &Path, profile: &NumericContractProfile) -> Result<()> {
    let path = output_dir
        .join("profiles")
        .join(format!("{}.json", profile.profile_id));
    if !path.exists() {
        fs::write(path, profile.to_pretty_json()?)?;
    }
    Ok(())
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<()> {
    let mut file = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

fn build_manifest(
    matrix: &ExperimentMatrix,
    args: &Args,
    matrix_bytes: &[u8],
    inputs: &HashMap<String, Vec<InputSample>>,
    profile_hashes: Vec<String>,
) -> Result<Manifest> {
    let mut canonical_inputs = Vec::new();
    let mut keys: Vec<_> = inputs.keys().collect();
    keys.sort();
    for key in keys {
        canonical_inputs.extend_from_slice(key.as_bytes());
        for sample in &inputs[key] {
            canonical_inputs.extend_from_slice(sample.sample_set.as_bytes());
            canonical_inputs.extend_from_slice(rational_string(&sample.value).as_bytes());
        }
    }
    let cargo_lock = fs::read("Cargo.lock").context("Cargo.lock ausente")?;
    let git_status = command_output("git", &["status", "--porcelain=v1"]);
    let git_diff = command_output("git", &["diff", "--binary", "HEAD"]);
    Ok(Manifest {
        schema_version: RESULT_SCHEMA_VERSION,
        matrix_id: matrix.matrix_id.clone(),
        seed: args.seed,
        matrix_path: args.matrix.display().to_string(),
        matrix_sha256: sha256_hex(matrix_bytes),
        effective_matrix_sha256: sha256_hex(&serde_json::to_vec(matrix)?),
        max_configurations: args.max_configurations,
        source_file_sha256: source_file_hashes()?,
        input_set_sha256: sha256_hex(&canonical_inputs),
        cargo_lock_sha256: sha256_hex(&cargo_lock),
        git_commit: command_output("git", &["rev-parse", "HEAD"]).trim().into(),
        git_dirty: !git_status.trim().is_empty(),
        git_diff_sha256: sha256_hex(format!("{git_status}\n{git_diff}").as_bytes()),
        rustc_version: command_output("rustc", &["--version"]).trim().into(),
        operating_system: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        process_peak_rss_bytes: peak_rss_bytes(),
        reference_precision_bits: matrix.reference_precision_bits,
        validation_precision_bits: matrix.validation_precision_bits,
        numeric_profile_hashes: profile_hashes,
        source_paper: "/Users/barbosa/Downloads/usenixsecurity26-sridhar.pdf".into(),
        source_artifact_doi: "10.5281/zenodo.21706296".into(),
        estimated_constraints_available: false,
        actual_constraints_available: false,
    })
}

fn source_file_hashes() -> Result<BTreeMap<String, String>> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()?;
    if !output.status.success() {
        bail!("cannot inventory reproducible source files");
    }
    let mut files = BTreeMap::new();
    for bytes in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let name = std::str::from_utf8(bytes)?;
        if [
            "crates/",
            "scripts/",
            "profiles/",
            "experiments/",
            "docs/",
            "Cargo.",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
            && Path::new(name).is_file()
        {
            files.insert(name.into(), sha256_hex(&fs::read(name)?));
        }
    }
    Ok(files)
}

#[cfg(target_os = "macos")]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` is valid writable memory and is only read after a
    // successful `getrusage` call.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if rc == 0 {
        // SAFETY: success guarantees that `usage` was initialized.
        let usage = unsafe { usage.assume_init() };
        u64::try_from(usage.ru_maxrss).ok()
    } else {
        None
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` is valid writable memory and is only read after a
    // successful `getrusage` call.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if rc == 0 {
        // SAFETY: success guarantees that `usage` was initialized.
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

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_else(|| "unavailable".into())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

fn rational_string(value: &BigRational) -> String {
    format!("{}/{}", value.numer(), value.denom())
}

fn rational_to_f64(value: &BigRational) -> f64 {
    value.to_f64().unwrap_or_else(|| {
        let float = Float::with_val(128, value.numer().to_f64().unwrap_or(0.0));
        (float / value.denom().to_f64().unwrap_or(1.0)).to_f64()
    })
}

fn accuracy_bits(error: &BigRational) -> Option<f64> {
    if error.is_zero() {
        None
    } else {
        Some(-rational_to_f64(error).abs().log2())
    }
}

fn rational_pow(value: &BigRational, exponent: usize) -> BigRational {
    (0..exponent).fold(BigRational::one(), |accumulator, _| accumulator * value)
}

fn rounding_tag(value: QuantizationRounding) -> &'static str {
    match value {
        QuantizationRounding::Floor => "floor",
        QuantizationRounding::TowardZero => "toward-zero",
        QuantizationRounding::NearestTiesToEven => "nearest-ties-to-even",
    }
}

fn semantic_class_tag(value: SemanticClass) -> &'static str {
    match value {
        SemanticClass::ExactReferenceSemantics => "exact-reference-semantics",
        SemanticClass::BoundedApproximation => "bounded-approximation",
        SemanticClass::UnboundedSemantics => "unbounded-semantics",
    }
}

fn decision_certification_tag(value: DecisionCertification) -> &'static str {
    match value {
        DecisionCertification::CertifiedStable => "certified-stable",
        DecisionCertification::Ambiguous => "ambiguous",
        DecisionCertification::ObservedSame => "observed-same",
        DecisionCertification::ObservedFlip => "observed-flip",
    }
}

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_results_never_become_certified() {
        assert_eq!(
            classify_decision(
                &BigRational::new(3.into(), 4.into()),
                &BigRational::new(1.into(), 2.into()),
                None,
                true,
            )
            .unwrap(),
            DecisionCertification::ObservedSame
        );
    }

    #[test]
    fn certified_interval_is_stable_or_ambiguous() {
        let value = BigRational::new(3.into(), 4.into());
        let threshold = BigRational::new(1.into(), 2.into());
        assert_eq!(
            classify_decision(
                &value,
                &threshold,
                Some(&BigRational::new(1.into(), 8.into())),
                true,
            )
            .unwrap(),
            DecisionCertification::CertifiedStable
        );
        assert_eq!(
            classify_decision(
                &value,
                &threshold,
                Some(&BigRational::new(1.into(), 3.into())),
                true,
            )
            .unwrap(),
            DecisionCertification::Ambiguous
        );
    }

    #[test]
    fn splitmix_is_deterministic() {
        let mut left = SplitMix64::new(7);
        let mut right = SplitMix64::new(7);
        assert_eq!(
            (0..16).map(|_| left.next()).collect::<Vec<_>>(),
            (0..16).map(|_| right.next()).collect::<Vec<_>>()
        );
    }
}

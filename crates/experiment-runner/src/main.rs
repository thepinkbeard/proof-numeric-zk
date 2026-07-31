use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use num_bigint::BigUint;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use numeric_core::{
    CertifiedDyadic, CompensatedNumber, Decision3, LimbNumber, NumericProfile, RoundingMode,
};
use numeric_reference::{
    decimal_ratio, logistic_trajectory_high_precision, rational_to_float, HighPrecisionConfig,
    HighPrecisionPoint,
};
use rug::Float;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    hint::black_box,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Csv,
    Json,
}

#[derive(Debug, Parser)]
#[command(about = "Executa o mapa logístico nos formatos numéricos do estudo")]
struct Args {
    #[arg(long, default_value_t = 100)]
    steps: usize,
    #[arg(long, default_value = "3.7")]
    r: String,
    /// Zero seleciona automaticamente `2*steps + 128` bits.
    #[arg(long, default_value_t = 0)]
    reference_bits: u32,
    #[arg(long, value_enum, default_value_t = OutputFormat::Csv)]
    output_format: OutputFormat,
    #[arg(long, default_value = "data/trajectory.csv")]
    output: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
struct Record {
    format: String,
    p: Option<usize>,
    k: Option<usize>,
    effective_bits: usize,
    rounding: String,
    r: String,
    x0: String,
    horizon: usize,
    iteration: usize,
    value: String,
    value_display: f64,
    reference_value: String,
    reference_bits: u32,
    reference_error_bound: String,
    absolute_error: String,
    absolute_error_display: f64,
    relative_error: Option<String>,
    ulp: String,
    discarded: Option<String>,
    residual: Option<String>,
    rounded_up: bool,
    overflow: bool,
    difference_fp32: f64,
    difference_fp64: f64,
    predicate_flip_025: bool,
    predicate_flip_050: bool,
    predicate_flip_075: bool,
    certified_lower: Option<String>,
    certified_upper: Option<String>,
    certified_radius_ulp: Option<String>,
    decision_025: Option<String>,
    decision_050: Option<String>,
    decision_075: Option<String>,
    unsound_certified_025: bool,
    unsound_certified_050: bool,
    unsound_certified_075: bool,
}

#[derive(Clone, Debug, Serialize)]
struct Summary {
    format: String,
    r: String,
    horizon: usize,
    maximum_absolute_error: f64,
    first_error_above_1e_6: Option<usize>,
    first_error_above_1e_9: Option<usize>,
    first_error_above_1e_12: Option<usize>,
    first_predicate_flip: Option<usize>,
    total_predicate_flips_025: usize,
    total_predicate_flips_050: usize,
    total_predicate_flips_075: usize,
    definite_decisions_025: usize,
    definite_decisions_050: usize,
    definite_decisions_075: usize,
    indeterminate_decisions_025: usize,
    indeterminate_decisions_050: usize,
    indeterminate_decisions_075: usize,
    unsound_certified_decisions: usize,
}

#[derive(Clone, Debug)]
struct CertifiedInterval {
    lower: BigRational,
    upper: BigRational,
    radius_ulp: BigUint,
}

#[derive(Clone, Debug)]
struct ApproxPoint {
    value: BigRational,
    discarded: Option<String>,
    residual: Option<String>,
    rounded_up: bool,
    overflow: bool,
    certified: Option<CertifiedInterval>,
}

fn parse_r(value: &str) -> Result<BigRational> {
    match value {
        "3.5" => Ok(decimal_ratio(7, 2)),
        "3.7" => Ok(decimal_ratio(37, 10)),
        "3.9" => Ok(decimal_ratio(39, 10)),
        "4.0" | "4" => Ok(decimal_ratio(4, 1)),
        _ => anyhow::bail!("r deve ser 3.5, 3.7, 3.9 ou 4.0"),
    }
}

fn ratio_parts(value: &BigRational) -> (BigUint, BigUint) {
    (
        value.numer().to_biguint().expect("r não negativo"),
        value.denom().to_biguint().expect("denominador positivo"),
    )
}

fn limb_trajectory<const P: usize, const K: usize>(
    x0: &BigRational,
    r: &BigRational,
    steps: usize,
    mode: RoundingMode,
) -> Result<Vec<ApproxPoint>> {
    let initial = LimbNumber::<P, K>::from_rational(x0, mode)?;
    let (r_num, r_den) = ratio_parts(r);
    let mut state = initial.value;
    let mut points = Vec::with_capacity(steps + 1);
    points.push(ApproxPoint {
        value: state.to_big_rational(),
        discarded: None,
        residual: None,
        rounded_up: initial.rounded_up,
        overflow: false,
        certified: None,
    });
    for _ in 0..steps {
        let complement = state.one_minus();
        let product = state.mul(&complement, mode)?;
        let scaled = product.value.mul_ratio(&r_num, &r_den, mode)?;
        state = scaled.value;
        points.push(ApproxPoint {
            value: state.to_big_rational(),
            discarded: Some(format!(
                "mul:{}/{};scale:{}/{}",
                product.discarded,
                LimbNumber::<P, K>::scale(),
                scaled.discarded,
                scaled.divisor
            )),
            residual: None,
            rounded_up: product.rounded_up || scaled.rounded_up,
            overflow: false,
            certified: None,
        });
    }
    Ok(points)
}

fn compensated_trajectory<const P: usize, const K: usize>(
    x0: &BigRational,
    r: &BigRational,
    steps: usize,
    mode: RoundingMode,
) -> Result<Vec<ApproxPoint>> {
    let initial = CompensatedNumber::<P, K>::from_rational(x0, mode)?;
    let mut state = initial.value;
    let mut points = Vec::with_capacity(steps + 1);
    points.push(ApproxPoint {
        value: state.to_big_rational(),
        discarded: None,
        residual: Some(state.residual().to_string()),
        rounded_up: initial.rounded_up,
        overflow: false,
        certified: None,
    });
    for _ in 0..steps {
        let next = state.logistic_step(r, mode)?;
        state = next.value;
        points.push(ApproxPoint {
            value: state.to_big_rational(),
            discarded: Some(next.exact_error.to_string()),
            residual: Some(state.residual().to_string()),
            rounded_up: next.rounded_up,
            overflow: false,
            certified: None,
        });
    }
    Ok(points)
}

fn cbd_trajectory(
    fractional_bits: usize,
    x0: &BigRational,
    r: &BigRational,
    steps: usize,
    mode: CbdTrajectoryMode,
) -> Result<Vec<ApproxPoint>> {
    let profile = NumericProfile::bounded_unit(fractional_bits)?;
    let mut state = CertifiedDyadic::from_exact(profile, x0)?;
    let mut points = Vec::with_capacity(steps + 1);
    for iteration in 0..=steps {
        points.push(ApproxPoint {
            value: state.center(),
            discarded: None,
            residual: Some(format!("radius_ulp={}", state.radius())),
            rounded_up: false,
            overflow: false,
            certified: Some(CertifiedInterval {
                lower: state.lower(),
                upper: state.upper(),
                radius_ulp: state.radius().clone(),
            }),
        });
        if iteration < steps {
            state = match mode {
                CbdTrajectoryMode::Lipschitz => state.logistic_step_lipschitz(r)?,
                CbdTrajectoryMode::LocalFused => state.logistic_step_local_bound(r)?,
                CbdTrajectoryMode::LocalSequential => {
                    state.logistic_step_sequential_local_bound(r)?
                }
            };
        }
    }
    Ok(points)
}

#[derive(Clone, Copy)]
enum CbdTrajectoryMode {
    Lipschitz,
    LocalFused,
    LocalSequential,
}

#[inline(never)]
fn fp32_step(x: f32, r: f32) -> f32 {
    let complement = black_box(1.0_f32 - black_box(x));
    let product = black_box(black_box(x) * black_box(complement));
    black_box(black_box(product) * black_box(r))
}

#[inline(never)]
fn fp64_step(x: f64, r: f64) -> f64 {
    let complement = black_box(1.0_f64 - black_box(x));
    let product = black_box(black_box(x) * black_box(complement));
    black_box(black_box(product) * black_box(r))
}

fn fp_trajectories(steps: usize, r: f64) -> (Vec<f32>, Vec<f64>) {
    let mut fp32 = vec![0.1_f32];
    let mut fp64 = vec![0.1_f64];
    for _ in 0..steps {
        fp32.push(fp32_step(*fp32.last().unwrap(), r as f32));
        fp64.push(fp64_step(*fp64.last().unwrap(), r));
    }
    (fp32, fp64)
}

fn fp_points_f32(values: &[f32]) -> Vec<ApproxPoint> {
    values
        .iter()
        .map(|&value| ApproxPoint {
            value: BigRational::from_float(value).expect("f32 finito"),
            discarded: None,
            residual: None,
            rounded_up: false,
            overflow: !value.is_finite(),
            certified: None,
        })
        .collect()
}

fn fp_points_f64(values: &[f64]) -> Vec<ApproxPoint> {
    values
        .iter()
        .map(|&value| ApproxPoint {
            value: BigRational::from_float(value).expect("f64 finito"),
            discarded: None,
            residual: None,
            rounded_up: false,
            overflow: !value.is_finite(),
            certified: None,
        })
        .collect()
}

fn predicate_flip(approx: &BigRational, reference: &Float, threshold: f64) -> bool {
    let approx_side = approx.to_f64().unwrap_or(f64::NAN) >= threshold;
    let reference_side = reference >= &Float::with_val(reference.prec(), threshold);
    approx_side != reference_side
}

fn interval_decision(interval: &CertifiedInterval, threshold: &BigRational) -> Decision3 {
    if interval.lower >= *threshold {
        Decision3::True
    } else if interval.upper < *threshold {
        Decision3::False
    } else {
        Decision3::Indeterminate
    }
}

fn decision_label(decision: Decision3) -> &'static str {
    match decision {
        Decision3::False => "false",
        Decision3::True => "true",
        Decision3::Indeterminate => "indeterminate",
    }
}

fn certified_decision_is_unsound(decision: Decision3, reference: &Float, threshold: f64) -> bool {
    let reference_side = reference >= &Float::with_val(reference.prec(), threshold);
    match decision {
        Decision3::False => reference_side,
        Decision3::True => !reference_side,
        Decision3::Indeterminate => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn append_records(
    output: &mut Vec<Record>,
    label: &str,
    p: Option<usize>,
    k: Option<usize>,
    effective_bits: usize,
    rounding: &str,
    r_label: &str,
    horizon: usize,
    reference_bits: u32,
    points: &[ApproxPoint],
    reference: &[HighPrecisionPoint],
    fp32: &[f32],
    fp64: &[f64],
) {
    let ulp = if effective_bits == 0 {
        "variable".to_owned()
    } else {
        format!("1/2^{effective_bits}")
    };
    for (iteration, (point, reference_point)) in points.iter().zip(reference).enumerate() {
        let certified_decision = |numerator: i64, denominator: i64| {
            point.certified.as_ref().map(|interval| {
                interval_decision(
                    interval,
                    &BigRational::new(numerator.into(), denominator.into()),
                )
            })
        };
        let decision_025 = certified_decision(1, 4);
        let decision_050 = certified_decision(1, 2);
        let decision_075 = certified_decision(3, 4);
        let approximate = rational_to_float(&point.value, reference_bits);
        let mut absolute_error = Float::with_val(reference_bits, &approximate);
        absolute_error -= &reference_point.value;
        absolute_error.abs_mut();
        let relative_error = if reference_point.value == 0 {
            None
        } else {
            let mut relative = absolute_error.clone();
            relative /= &reference_point.value;
            Some(relative.to_string_radix(10, Some(30)))
        };
        let display = point.value.to_f64().unwrap_or(f64::NAN);
        output.push(Record {
            format: label.to_owned(),
            p,
            k,
            effective_bits,
            rounding: rounding.to_owned(),
            r: r_label.to_owned(),
            x0: "1/10".to_owned(),
            horizon,
            iteration,
            value: point.value.to_string(),
            value_display: display,
            reference_value: reference_point.value.to_string_radix(10, Some(40)),
            reference_bits,
            reference_error_bound: reference_point
                .estimated_absolute_error
                .to_string_radix(10, Some(12)),
            absolute_error: absolute_error.to_string_radix(10, Some(30)),
            absolute_error_display: absolute_error.to_f64(),
            relative_error,
            ulp: ulp.clone(),
            discarded: point.discarded.clone(),
            residual: point.residual.clone(),
            rounded_up: point.rounded_up,
            overflow: point.overflow,
            difference_fp32: display - fp32[iteration] as f64,
            difference_fp64: display - fp64[iteration],
            predicate_flip_025: predicate_flip(&point.value, &reference_point.value, 0.25),
            predicate_flip_050: predicate_flip(&point.value, &reference_point.value, 0.5),
            predicate_flip_075: predicate_flip(&point.value, &reference_point.value, 0.75),
            certified_lower: point
                .certified
                .as_ref()
                .map(|interval| interval.lower.to_string()),
            certified_upper: point
                .certified
                .as_ref()
                .map(|interval| interval.upper.to_string()),
            certified_radius_ulp: point
                .certified
                .as_ref()
                .map(|interval| interval.radius_ulp.to_string()),
            decision_025: decision_025.map(decision_label).map(str::to_owned),
            decision_050: decision_050.map(decision_label).map(str::to_owned),
            decision_075: decision_075.map(decision_label).map(str::to_owned),
            unsound_certified_025: decision_025.is_some_and(|decision| {
                certified_decision_is_unsound(decision, &reference_point.value, 0.25)
            }),
            unsound_certified_050: decision_050.is_some_and(|decision| {
                certified_decision_is_unsound(decision, &reference_point.value, 0.5)
            }),
            unsound_certified_075: decision_075.is_some_and(|decision| {
                certified_decision_is_unsound(decision, &reference_point.value, 0.75)
            }),
        });
    }
}

fn summarize(records: &[Record]) -> Vec<Summary> {
    let mut groups: BTreeMap<&str, Vec<&Record>> = BTreeMap::new();
    for record in records {
        groups.entry(&record.format).or_default().push(record);
    }
    groups
        .into_iter()
        .map(|(format, rows)| {
            let first_above = |threshold: f64| {
                rows.iter()
                    .find(|row| row.absolute_error_display > threshold)
                    .map(|row| row.iteration)
            };
            let definite_count = |select: fn(&Record) -> &Option<String>| {
                rows.iter()
                    .filter(|row| {
                        select(row)
                            .as_deref()
                            .is_some_and(|value| value != "indeterminate")
                    })
                    .count()
            };
            let indeterminate_count = |select: fn(&Record) -> &Option<String>| {
                rows.iter()
                    .filter(|row| select(row).as_deref() == Some("indeterminate"))
                    .count()
            };
            Summary {
                format: format.to_owned(),
                r: rows[0].r.clone(),
                horizon: rows[0].horizon,
                maximum_absolute_error: rows
                    .iter()
                    .map(|row| row.absolute_error_display)
                    .fold(0.0, f64::max),
                first_error_above_1e_6: first_above(1e-6),
                first_error_above_1e_9: first_above(1e-9),
                first_error_above_1e_12: first_above(1e-12),
                first_predicate_flip: rows
                    .iter()
                    .find(|row| {
                        row.predicate_flip_025 || row.predicate_flip_050 || row.predicate_flip_075
                    })
                    .map(|row| row.iteration),
                total_predicate_flips_025: rows.iter().filter(|row| row.predicate_flip_025).count(),
                total_predicate_flips_050: rows.iter().filter(|row| row.predicate_flip_050).count(),
                total_predicate_flips_075: rows.iter().filter(|row| row.predicate_flip_075).count(),
                definite_decisions_025: definite_count(|row| &row.decision_025),
                definite_decisions_050: definite_count(|row| &row.decision_050),
                definite_decisions_075: definite_count(|row| &row.decision_075),
                indeterminate_decisions_025: indeterminate_count(|row| &row.decision_025),
                indeterminate_decisions_050: indeterminate_count(|row| &row.decision_050),
                indeterminate_decisions_075: indeterminate_count(|row| &row.decision_075),
                unsound_certified_decisions: rows
                    .iter()
                    .filter(|row| {
                        row.unsound_certified_025
                            || row.unsound_certified_050
                            || row.unsound_certified_075
                    })
                    .count(),
            }
        })
        .collect()
}

macro_rules! add_limb {
    ($records:expr, $p:expr, $k:expr, $label:expr, $mode:expr, $rounding:expr,
     $x0:expr, $r:expr, $steps:expr, $r_label:expr, $bits:expr, $reference:expr,
     $fp32:expr, $fp64:expr) => {{
        let points = limb_trajectory::<$p, $k>($x0, $r, $steps, $mode)?;
        append_records(
            $records,
            $label,
            Some($p),
            Some($k),
            $p * $k,
            $rounding,
            $r_label,
            $steps,
            $bits,
            &points,
            $reference,
            $fp32,
            $fp64,
        );
    }};
}

fn run(args: &Args) -> Result<Vec<Record>> {
    let x0 = decimal_ratio(1, 10);
    let r = parse_r(&args.r)?;
    let reference_bits = if args.reference_bits == 0 {
        (args.steps.saturating_mul(2).saturating_add(128))
            .max(256)
            .try_into()
            .unwrap_or(u32::MAX)
    } else {
        args.reference_bits
    };
    let reference = logistic_trajectory_high_precision(
        &x0,
        &r,
        args.steps,
        &HighPrecisionConfig {
            precision_bits: reference_bits,
        },
    )?;
    let (fp32, fp64) = fp_trajectories(args.steps, r.to_f64().context("r não cabe em f64")?);
    let mut records = Vec::new();

    let fp32_points = fp_points_f32(&fp32);
    append_records(
        &mut records,
        "f32",
        None,
        None,
        24,
        "IEEE-754 operations separated; FMA disabled by construction",
        &args.r,
        args.steps,
        reference_bits,
        &fp32_points,
        &reference,
        &fp32,
        &fp64,
    );
    let fp64_points = fp_points_f64(&fp64);
    append_records(
        &mut records,
        "f64",
        None,
        None,
        53,
        "IEEE-754 operations separated; FMA disabled by construction",
        &args.r,
        args.steps,
        reference_bits,
        &fp64_points,
        &reference,
        &fp32,
        &fp64,
    );

    for (mode, suffix, rounding) in [
        (RoundingMode::Truncate, "truncate", "truncate"),
        (
            RoundingMode::NearestTiesToEven,
            "nearest_even",
            "nearest-ties-to-even",
        ),
    ] {
        add_limb!(
            &mut records,
            16,
            1,
            &format!("Q16.16_{suffix}"),
            mode,
            rounding,
            &x0,
            &r,
            args.steps,
            &args.r,
            reference_bits,
            &reference,
            &fp32,
            &fp64
        );
        add_limb!(
            &mut records,
            32,
            1,
            &format!("Q32.32_{suffix}"),
            mode,
            rounding,
            &x0,
            &r,
            args.steps,
            &args.r,
            reference_bits,
            &reference,
            &fp32,
            &fp64
        );
        add_limb!(
            &mut records,
            16,
            2,
            &format!("limbs_16x2_{suffix}"),
            mode,
            rounding,
            &x0,
            &r,
            args.steps,
            &args.r,
            reference_bits,
            &reference,
            &fp32,
            &fp64
        );
        add_limb!(
            &mut records,
            16,
            3,
            &format!("limbs_16x3_{suffix}"),
            mode,
            rounding,
            &x0,
            &r,
            args.steps,
            &args.r,
            reference_bits,
            &reference,
            &fp32,
            &fp64
        );
        add_limb!(
            &mut records,
            16,
            4,
            &format!("limbs_16x4_{suffix}"),
            mode,
            rounding,
            &x0,
            &r,
            args.steps,
            &args.r,
            reference_bits,
            &reference,
            &fp32,
            &fp64
        );
    }

    let compensated = [
        (
            "limbs_16x1_compensated",
            compensated_trajectory::<16, 1>(&x0, &r, args.steps, RoundingMode::NearestTiesToEven)?,
            1,
            32,
        ),
        (
            "limbs_16x2_compensated",
            compensated_trajectory::<16, 2>(&x0, &r, args.steps, RoundingMode::NearestTiesToEven)?,
            2,
            64,
        ),
        (
            "limbs_16x3_compensated",
            compensated_trajectory::<16, 3>(&x0, &r, args.steps, RoundingMode::NearestTiesToEven)?,
            3,
            96,
        ),
        (
            "limbs_16x4_compensated",
            compensated_trajectory::<16, 4>(&x0, &r, args.steps, RoundingMode::NearestTiesToEven)?,
            4,
            128,
        ),
    ];
    for (label, points, k, bits) in compensated {
        append_records(
            &mut records,
            label,
            Some(16),
            Some(k),
            bits,
            "nearest-ties-to-even on S^2 grid",
            &args.r,
            args.steps,
            reference_bits,
            &points,
            &reference,
            &fp32,
            &fp64,
        );
    }

    for (mode, suffix, description) in [
        (
            CbdTrajectoryMode::Lipschitz,
            "lipschitz",
            "nearest-ties-to-even fused; certified global Lipschitz radius",
        ),
        (
            CbdTrajectoryMode::LocalFused,
            "local",
            "nearest-ties-to-even fused; certified local Taylor radius",
        ),
        (
            CbdTrajectoryMode::LocalSequential,
            "sequential_local",
            "nearest-ties-to-even sequential; certified local Taylor radius",
        ),
    ] {
        for bits in [16_usize, 32, 48, 64] {
            let points = cbd_trajectory(bits, &x0, &r, args.steps, mode)?;
            append_records(
                &mut records,
                &format!("cbd_{bits}_{suffix}"),
                Some(16),
                Some(bits / 16),
                bits,
                description,
                &args.r,
                args.steps,
                reference_bits,
                &points,
                &reference,
                &fp32,
                &fp64,
            );
        }
    }
    Ok(records)
}

fn write_records(path: &Path, format: OutputFormat, records: &[Record]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match format {
        OutputFormat::Csv => {
            let mut writer = csv::Writer::from_path(path)?;
            for record in records {
                writer.serialize(record)?;
            }
            writer.flush()?;
        }
        OutputFormat::Json => {
            serde_json::to_writer_pretty(File::create(path)?, records)?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let records = run(&args)?;
    write_records(&args.output, args.output_format, &records)?;
    let summary_path = args.output.with_extension("summary.json");
    serde_json::to_writer_pretty(File::create(&summary_path)?, &summarize(&records))?;
    println!(
        "{} linhas gravadas em {}; resumo em {}",
        records.len(),
        args.output.display(),
        summary_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separated_fp_steps_match_written_operation_order() {
        let x = 0.1_f32;
        let r = 3.7_f32;
        let complement = 1.0_f32 - x;
        let product = x * complement;
        assert_eq!(fp32_step(x, r).to_bits(), (product * r).to_bits());
    }

    #[test]
    fn short_experiment_contains_every_required_family() {
        let args = Args {
            steps: 2,
            r: "3.7".to_owned(),
            reference_bits: 256,
            output_format: OutputFormat::Csv,
            output: PathBuf::from("unused.csv"),
        };
        let records = run(&args).unwrap();
        let labels: std::collections::BTreeSet<_> = records
            .iter()
            .map(|record| record.format.as_str())
            .collect();
        assert!(labels.contains("f32"));
        assert!(labels.contains("f64"));
        assert!(labels.contains("Q16.16_truncate"));
        assert!(labels.contains("Q32.32_nearest_even"));
        assert!(labels.contains("limbs_16x4_nearest_even"));
        assert!(labels.contains("limbs_16x2_compensated"));
        assert!(labels.contains("cbd_32_lipschitz"));
        assert!(labels.contains("cbd_32_local"));
        assert!(labels.contains("cbd_32_sequential_local"));
    }

    #[test]
    fn cbd_never_emits_an_unsound_definite_decision_in_short_run() {
        let args = Args {
            steps: 30,
            r: "3.9".to_owned(),
            reference_bits: 256,
            output_format: OutputFormat::Csv,
            output: PathBuf::from("unused.csv"),
        };
        let records = run(&args).unwrap();
        assert!(records
            .iter()
            .filter(|record| record.format.starts_with("cbd_"))
            .all(|record| !record.unsound_certified_025
                && !record.unsound_certified_050
                && !record.unsound_certified_075));
    }
}

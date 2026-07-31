use anyhow::{Context, Result};
use clap::Parser;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use numeric_core::{CertifiedDyadic, Decision3, NumericProfile};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Parser)]
#[command(about = "Avalia CBD em score logístico e proximidade ECEF")]
struct Args {
    #[arg(long, default_value = "results/adult_logistic_model.json")]
    adult_model: PathBuf,
    #[arg(long, default_value = "results/cbd_workloads.json")]
    output: PathBuf,
    #[arg(long, default_value_t = 2_000)]
    ecef_cases: usize,
}

#[derive(Debug, Deserialize)]
struct AdultArtifact {
    weights: Vec<f64>,
    bias: f64,
    cases: Vec<AdultCase>,
}

#[derive(Debug, Deserialize)]
struct AdultCase {
    features: Vec<f64>,
    label: u8,
    reference_score: f64,
}

#[derive(Clone, Debug, Serialize)]
struct ProfileMetrics {
    fractional_bits: usize,
    cases: usize,
    definite: usize,
    indeterminate: usize,
    unsound_definite: usize,
    center_decision_disagreements: usize,
    maximum_interval_width: f64,
}

#[derive(Debug, Serialize)]
struct AdultResult {
    dataset: &'static str,
    features: usize,
    cases: usize,
    source_model_label_accuracy: f64,
    profiles: Vec<ProfileMetrics>,
}

#[derive(Debug, Serialize)]
struct EcefResult {
    kernel: &'static str,
    radius_km: f64,
    cases: usize,
    profiles: Vec<ProfileMetrics>,
}

#[derive(Debug, Serialize)]
struct WorkloadResult {
    schema_version: u32,
    adult: AdultResult,
    ecef: EcefResult,
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("valor finito")
}

fn signed_profile(bits: usize) -> NumericProfile {
    NumericProfile::new(
        format!("cbd-signed-workload-{bits}"),
        1,
        bits,
        bits + 7,
        BigRational::from_integer((-64).into()),
        BigRational::from_integer(64.into()),
        vec![BigRational::from_integer(0.into())],
        match bits {
            16 => Some(32),
            32 => Some(48),
            48 => Some(64),
            _ => None,
        },
    )
    .expect("perfil assinado válido")
}

fn is_unsound(decision: Decision3, reference: bool) -> bool {
    matches!(
        (decision, reference),
        (Decision3::True, false) | (Decision3::False, true)
    )
}

fn adult_metrics(artifact: &AdultArtifact, bits: usize) -> Result<ProfileMetrics> {
    let profile = signed_profile(bits);
    let zero = BigRational::from_integer(0.into());
    let bias = CertifiedDyadic::from_exact(profile.clone(), &rational(artifact.bias))?;
    let encoded_weights = artifact
        .weights
        .iter()
        .map(|weight| CertifiedDyadic::from_exact(profile.clone(), &rational(*weight)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut definite = 0;
    let mut indeterminate = 0;
    let mut unsound_definite = 0;
    let mut center_decision_disagreements = 0;
    let mut maximum_interval_width = 0.0_f64;

    for case in &artifact.cases {
        let terms = case
            .features
            .iter()
            .zip(&encoded_weights)
            .map(|(feature, weight)| {
                Ok((
                    CertifiedDyadic::from_exact(profile.clone(), &rational(*feature))?,
                    weight.clone(),
                ))
            })
            .collect::<Result<Vec<_>, numeric_core::CbdError>>()?;
        let score = CertifiedDyadic::linear_score(&bias, &terms)?;
        let exact_score = case
            .features
            .iter()
            .zip(&artifact.weights)
            .fold(rational(artifact.bias), |acc, (feature, weight)| {
                acc + rational(*feature) * rational(*weight)
            });
        let reference = exact_score >= zero;
        let decision = score.ge_threshold(&zero);
        match decision {
            Decision3::Indeterminate => indeterminate += 1,
            _ => {
                definite += 1;
                unsound_definite += usize::from(is_unsound(decision, reference));
            }
        }
        center_decision_disagreements += usize::from((score.center() >= zero) != reference);
        maximum_interval_width = maximum_interval_width.max(
            (score.upper() - score.lower())
                .to_f64()
                .unwrap_or(f64::INFINITY),
        );
    }
    Ok(ProfileMetrics {
        fractional_bits: bits,
        cases: artifact.cases.len(),
        definite,
        indeterminate,
        unsound_definite,
        center_decision_disagreements,
        maximum_interval_width,
    })
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

fn ecef_cases(count: usize, threshold_angle: f64) -> Vec<([f64; 3], [f64; 3])> {
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
            // Concentra metade dos casos muito perto do limiar.
            let factor = if index % 2 == 0 {
                1.0 + (next_unit() - 0.5) * 2e-4
            } else {
                0.5 + next_unit()
            };
            let (target_latitude, target_longitude) =
                destination(latitude, longitude, bearing, threshold_angle * factor);
            (
                unit_vector(latitude, longitude),
                unit_vector(target_latitude, target_longitude),
            )
        })
        .collect()
}

fn ecef_metrics(
    cases: &[([f64; 3], [f64; 3])],
    threshold: &BigRational,
    bits: usize,
) -> Result<ProfileMetrics> {
    let profile = signed_profile(bits);
    let zero = CertifiedDyadic::from_exact(profile.clone(), &BigRational::from_integer(0.into()))?;
    let mut definite = 0;
    let mut indeterminate = 0;
    let mut unsound_definite = 0;
    let mut center_decision_disagreements = 0;
    let mut maximum_interval_width = 0.0_f64;

    for (left, right) in cases {
        let mut squared_distance = zero.clone();
        let mut exact_distance = BigRational::from_integer(0.into());
        for (a, b) in left.iter().zip(right) {
            let exact_a = rational(*a);
            let exact_b = rational(*b);
            let encoded_a = CertifiedDyadic::from_exact(profile.clone(), &exact_a)?;
            let encoded_b = CertifiedDyadic::from_exact(profile.clone(), &exact_b)?;
            let difference = encoded_a.sub(&encoded_b)?;
            squared_distance = squared_distance.add(&difference.mul(&difference)?)?;
            let exact_difference = exact_a - exact_b;
            exact_distance += &exact_difference * &exact_difference;
        }
        let reference = exact_distance <= *threshold;
        let decision = squared_distance.le_threshold(threshold);
        match decision {
            Decision3::Indeterminate => indeterminate += 1,
            _ => {
                definite += 1;
                unsound_definite += usize::from(is_unsound(decision, reference));
            }
        }
        center_decision_disagreements +=
            usize::from((squared_distance.center() <= *threshold) != reference);
        maximum_interval_width = maximum_interval_width.max(
            (squared_distance.upper() - squared_distance.lower())
                .to_f64()
                .unwrap_or(f64::INFINITY),
        );
    }
    Ok(ProfileMetrics {
        fractional_bits: bits,
        cases: cases.len(),
        definite,
        indeterminate,
        unsound_definite,
        center_decision_disagreements,
        maximum_interval_width,
    })
}

fn main() -> Result<()> {
    let args = Args::parse();
    let artifact: AdultArtifact = serde_json::from_slice(
        &fs::read(&args.adult_model)
            .with_context(|| format!("leitura de {}", args.adult_model.display()))?,
    )?;
    anyhow::ensure!(
        artifact
            .cases
            .iter()
            .all(|case| case.features.len() == artifact.weights.len()),
        "dimensão incompatível no artefato Adult"
    );
    let source_model_label_accuracy = artifact
        .cases
        .iter()
        .filter(|case| (case.reference_score >= 0.0) == (case.label == 1))
        .count() as f64
        / artifact.cases.len() as f64;

    let radius_km: f64 = 100.0;
    let earth_radius_km: f64 = 6_371.0;
    let threshold_angle = radius_km / earth_radius_km;
    let threshold_chord_squared = 2.0 - 2.0 * threshold_angle.cos();
    let cases = ecef_cases(args.ecef_cases, threshold_angle);
    let threshold = rational(threshold_chord_squared);

    let bits = [16_usize, 32, 48, 64];
    let result = WorkloadResult {
        schema_version: 1,
        adult: AdultResult {
            dataset: "UCI Adult numeric features",
            features: artifact.weights.len(),
            cases: artifact.cases.len(),
            source_model_label_accuracy,
            profiles: bits
                .iter()
                .map(|bits| adult_metrics(&artifact, *bits))
                .collect::<Result<Vec<_>>>()?,
        },
        ecef: EcefResult {
            kernel: "squared chord distance over normalized ECEF vectors",
            radius_km,
            cases: cases.len(),
            profiles: bits
                .iter()
                .map(|bits| ecef_metrics(&cases, &threshold, *bits))
                .collect::<Result<Vec<_>>>()?,
        },
    };
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, serde_json::to_vec_pretty(&result)?)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

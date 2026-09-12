//! Small reproducible, off-circuit audit. Not the preregistered timing campaign.
use anyhow::{bail, Result};
use clap::Parser;
use num_bigint::BigInt;
use num_rational::BigRational;
use numeric_approximations::{generate_sigmoid_certificate, prepare_sigmoid_contract};
use numeric_core::{
    exact_ecef_squared_distance, select_checked_precision, CertificateError, CheckedEnclosure,
    ContractPrecisionPolicy, Decision3, FixedPointFormat, OverflowMode, QuantizationRounding,
    CONTRACT_PRECISION_LADDER,
};
use numeric_reference::evaluate_transcendental;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::Instant,
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    output_dir: PathBuf,
}
fn q(n: i64, d: i64) -> BigRational {
    BigRational::new(n.into(), d.into())
}
fn format(bits: u16) -> FixedPointFormat {
    FixedPointFormat {
        integer_bits: 8,
        fractional_bits: bits,
        signed: true,
        rounding: QuantizationRounding::NearestTiesToEven,
        overflow: OverflowMode::Reject,
    }
}
fn write(path: PathBuf, value: &impl serde::Serialize) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}
fn main() -> Result<()> {
    let args = Args::parse();
    // Atomic reservation, no silent overwrite of an earlier audit.
    fs::create_dir(&args.output_dir)?;
    let mut contracts = BTreeMap::new();
    let setup = Instant::now();
    for bits in CONTRACT_PRECISION_LADDER {
        contracts.insert(bits, prepare_sigmoid_contract(format(bits), 4)?);
    }
    let setup_ns = setup.elapsed().as_nanos();
    write(args.output_dir.join("contracts.json"), &contracts)?;
    let sigmoid_inputs = [q(-3, 4), q(-1, 4), q(1, 2), q(7, 8)];
    let left = [q(1, 2) + q(1, 1i64 << 40), q(0, 1), q(0, 1)];
    let right = [q(0, 1), q(0, 1), q(0, 1)];
    let distance = exact_ecef_squared_distance(&left, &right);
    let exact_margin = q(1, 4) - &distance; // d^2 <= r^2 iff margin >=0.
    let mut rows = Vec::new();
    let mut certificates = Vec::new();
    for policy in [
        ContractPrecisionPolicy::Fixed,
        ContractPrecisionPolicy::Sequential,
        ContractPrecisionPolicy::MarginDirected,
        ContractPrecisionPolicy::PreviewAll,
    ] {
        for workload in ["ecef-boundary-control", "sigmoid-average-control"] {
            let threshold = if workload.starts_with("ecef") {
                q(0, 1)
            } else {
                q(3, 5)
            };
            let mut reference_lower = q(0, 1);
            let mut reference_upper = q(0, 1);
            if workload.starts_with("ecef") {
                reference_lower = exact_margin.clone();
                reference_upper = exact_margin.clone();
            } else {
                for input in &sigmoid_inputs {
                    let value =
                        evaluate_transcendental(numeric_core::FunctionId::Sigmoid, input, 512)?;
                    reference_lower += value.lower_rational() / BigInt::from(4);
                    reference_upper += value.upper_rational() / BigInt::from(4);
                }
            }
            let result = select_checked_precision(policy, 16, &threshold, |bits| {
                if workload.starts_with("ecef") {
                    let f = format(bits);
                    let mut squared = CheckedEnclosure::exact(q(0, 1));
                    for (a, b) in left.iter().zip(&right) {
                        let d = CheckedEnclosure::quantized_input(a, &f)?
                            .sub(&CheckedEnclosure::quantized_input(b, &f)?)
                            .regrid(&f)?;
                        squared = squared.add(&d.square().regrid(&f)?).regrid(&f)?;
                    }
                    Ok(CheckedEnclosure::exact(q(1, 4)).sub(&squared).regrid(&f)?)
                } else {
                    let contract = &contracts[&bits];
                    let mut sum = CheckedEnclosure::exact(q(0, 1));
                    for input in &sigmoid_inputs {
                        let certificate = generate_sigmoid_certificate(contract, input)?;
                        let (checked, bound) = contract.check(input, &certificate)?;
                        certificates.push(json!({"precision":bits,"certificate":certificate,"checked_bound":bound}));
                        sum = sum.add(&checked);
                    }
                    Ok::<_, CertificateError>(sum.scale(&q(1, 4)))
                }
            })?;
            let decision = result.enclosure.decision(&threshold);
            if (decision == Decision3::True && reference_lower < threshold)
                || (decision == Decision3::False && reference_upper >= threshold)
            {
                bail!("certified decision contradicts directed reference");
            }
            if !result.enclosure.contains(&reference_lower)
                || !result.enclosure.contains(&reference_upper)
            {
                bail!("directed reference escaped checked enclosure");
            }
            rows.push(json!({"workload":workload,"policy":policy,"selected_precision":result.selected_precision,
                "decision":decision,"lower":result.enclosure.lower().to_string(),"upper":result.enclosure.upper().to_string(),
                "threshold":threshold.to_string(),"reference_lower":reference_lower.to_string(),"reference_upper":reference_upper.to_string(),
                "evaluations":result.evaluations,"total_selection_ns":result.total_selection_ns,
                "proof_time_ns":null,"verify_time_ns":null,"constraints":null,"commitment_bound":false}));
        }
    }
    write(args.output_dir.join("raw.json"), &rows)?;
    write(args.output_dir.join("certificates.json"), &certificates)?;
    let protocol = fs::read("experiments/cdad_contract_protocol_v1.json")?;
    write(
        args.output_dir.join("protocol.json"),
        &serde_json::from_slice::<serde_json::Value>(&protocol)?,
    )?;
    let tracked = std::process::Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()?;
    let mut source_hashes = BTreeMap::new();
    for name in tracked.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8(name.to_vec())?;
        if path.starts_with("crates/")
            || path.starts_with("experiments/")
            || path.starts_with("Cargo.")
        {
            if let Ok(bytes) = fs::read(&path) {
                source_hashes.insert(path, format!("{:x}", Sha256::digest(bytes)));
            }
        }
    }
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()?;
    let toolchain = std::process::Command::new("rustc")
        .arg("--version")
        .output()?;
    let protocol_hash = format!("{:x}", Sha256::digest(&protocol));
    let lock = fs::read("Cargo.lock")?;
    let lock_hash = format!("{:x}", Sha256::digest(&lock));
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    write(
        args.output_dir.join("manifest.json"),
        &json!({"schema_version":1,
        "run_kind":"deterministic-audit-smoke-not-performance-campaign","protocol_sha256":protocol_hash,
        "cargo_lock_sha256":lock_hash,"git_commit":String::from_utf8_lossy(&commit.stdout).trim(),
        "git_dirty": !dirty.stdout.is_empty(), "source_file_sha256": source_hashes,
        "rustc": String::from_utf8_lossy(&toolchain.stdout).trim(),
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "contract_preparation_ns":setup_ns,"canonical_inputs":{"ecef_left":left,"ecef_right":right,"sigmoid":sigmoid_inputs},
        "reference_precision_bits":512,"relation_provenance":"our-extension","rows":rows.len(),
        "cryptographic_guarantee":false,"performance_claim":false}),
    )?;
    Ok(())
}

//! Reproducible qualification data. No measured proof costs are fabricated.
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use experiment_runner::{qualification_trajectory, QualificationState};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive};
use numeric_reference::directed_dynamic_trajectory;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Select {
        #[arg(long)]
        x0: String,
        #[arg(long)]
        r: String,
        #[arg(long)]
        threshold: String,
        #[arg(long)]
        steps: usize,
        #[arg(long, default_value = "sequential")]
        policy: String,
        #[arg(long, default_value_t = 16)]
        precision: usize,
    },
    Generate {
        #[arg(long, default_value = "experiments/qualification_v1.json")]
        matrix: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
    },
    Evaluate {
        #[arg(long)]
        inputs_dir: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        /// Explicit development subset, never marked as the full campaign.
        #[arg(long)]
        max_inputs: Option<usize>,
        #[arg(long)]
        max_horizon: Option<usize>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
struct Matrix {
    schema_version: u32,
    protocol_id: String,
    generator: String,
    seeds: Vec<u64>,
    inputs_per_seed: usize,
    horizons: Vec<usize>,
    precision_ladder: Vec<usize>,
    logistic_parameters: Vec<String>,
    affine_parameters: Vec<[String; 2]>,
    logistic_thresholds: Vec<String>,
    affine_thresholds: Vec<String>,
    reference_max_bits: u32,
    public_inputs: bool,
    proof_horizons: Vec<usize>,
    timing_rounds: usize,
}
#[derive(Clone, Serialize, Deserialize)]
struct Input {
    id: String,
    set: String,
    seed: Option<u64>,
    value: String,
}
#[derive(Serialize)]
struct Row {
    input_id: String,
    input_set: String,
    workload: String,
    parameter: String,
    innovation: Option<String>,
    family: String,
    precision: usize,
    horizon: usize,
    threshold: String,
    profile_hash: String,
    lower: String,
    upper: String,
    reference_lower: String,
    reference_upper: String,
    reference_bits: u32,
    decision: String,
    reference_decision: Option<bool>,
    predicate_flip: Option<bool>,
    certified: bool,
    first_abstention: Option<usize>,
    center_absolute_error: f64,
    enclosure_width: f64,
    evaluations: Vec<usize>,
    evaluation_ns: Vec<u128>,
    selection_ns: u128,
    proof_attempts: Option<usize>,
    proof_cost_ns: Option<u128>,
    constraints: Option<usize>,
}
fn q(s: &str) -> Result<BigRational> {
    s.parse()
        .map_err(|_| anyhow::anyhow!("invalid rational {s}"))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    let f = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(f, value)?;
    Ok(())
}
fn reserve(path: &Path) -> Result<()> {
    fs::create_dir(path).context("output directory must be new and parent must exist")
}
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}
fn validate(m: &Matrix) -> Result<()> {
    if m.schema_version != 1
        || m.generator != "splitmix64-v1"
        || m.precision_ladder != [16, 32, 48, 64]
        || m.horizons.is_empty()
        || m.horizons.iter().any(|h| *h == 0 || *h > 1000)
        || m.reference_max_bits != 16384
        || !m.public_inputs
    {
        bail!("unsupported matrix");
    }
    if m.inputs_per_seed==0 || m.inputs_per_seed>1000 || m.seeds.is_empty()
        || m.logistic_parameters != ["7/2","37/10","39/10","4"]
        || m.affine_parameters != [["1/2".to_string(),"3/4".to_string()],["7/10".to_string(),"1/2".to_string()]]
        || m.logistic_thresholds != ["1/4","1/2","3/4"]
        || m.affine_thresholds != ["1","5/4","3/2"] {bail!("parameters differ from qualification-v1 contract; version a new protocol");}
    Ok(())
}
fn generate(matrix: &Path, output: &Path) -> Result<()> {
    let m: Matrix = serde_json::from_slice(&fs::read(matrix)?)?;
    validate(&m)?;
    reserve(output)?;
    let mut inputs = vec![Input {
        id: "historical-0".into(),
        set: "historical".into(),
        seed: None,
        value: "1/10".into(),
    }];
    for seed in &m.seeds {
        let mut state = *seed;
        for i in 0..m.inputs_per_seed {
            let mut j = splitmix(&mut state);
            while j == 0 {
                j = splitmix(&mut state);
            }
            inputs.push(Input {
                id: format!("seed-{seed}-{i}"),
                set: "sample".into(),
                seed: Some(*seed),
                value: BigRational::new(j.into(), BigInt::one() << 64).to_string(),
            });
        }
    }
    for (i, x) in ["0", "1", "1/2", "1/131072", "3/131072", "65535/65536"]
        .iter()
        .enumerate()
    {
        inputs.push(Input {
            id: format!("adversarial-{i}"),
            set: "adversarial".into(),
            seed: None,
            value: (*x).into(),
        });
    }
    save(&output.join("matrix.json"), &m)?;
    save(&output.join("inputs.json"), &inputs)?;
    save(
        &output.join("manifest.json"),
        &json!({"stage":"inputs-complete","schema_version":1,"matrix_hash":digest(&fs::read(output.join("matrix.json"))?),"inputs_hash":digest(&fs::read(output.join("inputs.json"))?),"count":inputs.len()}),
    )
}
fn decision(s: &QualificationState, t: &BigRational) -> Option<bool> {
    if s.lower >= *t {
        Some(true)
    } else if s.upper < *t {
        Some(false)
    } else {
        None
    }
}
fn label(d: Option<bool>) -> String {
    match d {
        Some(true) => "True",
        Some(false) => "False",
        None => "Indeterminate",
    }
    .into()
}
fn reference(
    x: &BigRational,
    a: &BigRational,
    c: Option<&BigRational>,
    n: usize,
    thresholds: &[BigRational],
    max: u32,
) -> Result<(u32, Vec<QualificationState>)> {
    let mut bits = ((2 * n + 128).max(256) as u32).min(max);
    loop {
        let values = directed_dynamic_trajectory(x, a, c, n, bits)
            .map_err(anyhow::Error::msg)?
            .into_iter()
            .map(|(lower, upper)| QualificationState { lower, upper })
            .collect::<Vec<_>>();
        if bits == max
            || values
                .iter()
                .all(|v| thresholds.iter().all(|t| decision(v, t).is_some()))
        {
            return Ok((bits, values));
        }
        bits = (bits * 2).min(max);
    }
}
fn selected(policy: &str, states: &[QualificationState], threshold: &BigRational) -> Vec<usize> {
    if policy == "preview-all" {
        return vec![0, 1, 2, 3];
    }
    let mut indices = vec![0];
    let mut i = 0;
    while decision(&states[i], threshold).is_none() && i < 3 {
        let mut next = i + 1;
        if policy == "margin-directed" {
            let center = (&states[i].lower + &states[i].upper) / BigInt::from(2);
            let margin = (center - threshold).abs();
            let radius = (&states[i].upper - &states[i].lower) / BigInt::from(2);
            while next < 3 && &radius / BigInt::from(1u64 << (16 * (next - i))) >= margin {
                next += 1;
            }
        }
        i = next;
        indices.push(i);
    }
    indices
}
fn source_manifest() -> Result<serde_json::Value> {
    let list = std::process::Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()?;
    let mut hashes = BTreeMap::new();
    for p in list.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let name = String::from_utf8(p.to_vec())?;
        if ["crates/", "scripts/", "experiments/", "Cargo."]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            if let Ok(b) = fs::read(&name) {
                hashes.insert(name, digest(&b));
            }
        }
    }
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    let rust = std::process::Command::new("rustc")
        .arg("--version")
        .output()?;
    Ok(
        json!({"source_hashes":hashes,"git_commit":String::from_utf8_lossy(&commit.stdout).trim(),"rustc":String::from_utf8_lossy(&rust.stdout).trim(),"os":std::env::consts::OS,"arch":std::env::consts::ARCH}),
    )
}
fn evaluate(
    inputs_dir: &Path,
    output: &Path,
    max_inputs: Option<usize>,
    max_horizon: Option<usize>,
) -> Result<()> {
    let m: Matrix = serde_json::from_slice(&fs::read(inputs_dir.join("matrix.json"))?)?;
    validate(&m)?;
    let input_manifest:serde_json::Value=serde_json::from_slice(&fs::read(inputs_dir.join("manifest.json"))?)?;
    if input_manifest["matrix_hash"]!=digest(&fs::read(inputs_dir.join("matrix.json"))?)
        || input_manifest["inputs_hash"]!=digest(&fs::read(inputs_dir.join("inputs.json"))?) {bail!("input artifact hash mismatch");}
    let mut inputs: Vec<Input> =
        serde_json::from_slice(&fs::read(inputs_dir.join("inputs.json"))?)?;
    if let Some(n) = max_inputs {
        inputs.truncate(n);
    }
    let horizons: Vec<_> = m
        .horizons
        .iter()
        .copied()
        .filter(|h| max_horizon.map_or(true, |max| *h <= max))
        .collect();
    if inputs.is_empty() || horizons.is_empty() {
        bail!("empty evaluation");
    }
    reserve(output)?;
    save(&output.join("matrix.json"), &m)?;
    save(&output.join("inputs.json"), &inputs)?;
    save(
        &output.join("started.json"),
        &json!({"status":"running","max_inputs":max_inputs,"max_horizon":max_horizon,"environment":source_manifest()?}),
    )?;
    let mut raw = BufWriter::new(File::create(output.join("raw.jsonl"))?);
    let mut trajectories = csv::Writer::from_path(output.join("trajectories.csv"))?;
    trajectories.write_record([
        "input_id",
        "workload",
        "parameter",
        "family",
        "precision",
        "iteration",
        "center",
        "lower",
        "upper",
        "reference_center",
        "error",
        "width",
    ])?;
    let mut rejected = BufWriter::new(File::create(output.join("rejected.jsonl"))?);
    let mut rows = 0usize;
    let mut rejected_count = 0usize;
    let n = *horizons.iter().max().unwrap();
    let mut workloads: Vec<(String, String, Option<String>, Vec<String>)> = m
        .logistic_parameters
        .iter()
        .map(|r| {
            (
                "logistic".into(),
                r.clone(),
                None,
                m.logistic_thresholds.clone(),
            )
        })
        .collect();
    workloads.extend(m.affine_parameters.iter().map(|v| {
        (
            "affine".into(),
            v[0].clone(),
            Some(v[1].clone()),
            m.affine_thresholds.clone(),
        )
    }));
    for input in &inputs {
        let x = q(&input.value)?;
        for (workload, param, innovation, threshold_text) in &workloads {
            let a = q(param)?;
            let c = innovation.as_deref().map(q).transpose()?;
            let thresholds = threshold_text
                .iter()
                .map(|s| q(s))
                .collect::<Result<Vec<_>>>()?;
            let (ref_bits, refs) =
                reference(&x, &a, c.as_ref(), n, &thresholds, m.reference_max_bits)?;
            let mut variants = Vec::new();
            for bits in &m.precision_ladder {
                for family in ["fixed-truncate", "fixed-rne", "cbd-local", "cbd-sequential"] {
                    if workload == "affine" && family == "cbd-sequential" {
                        continue;
                    }
                    variants.push((family, *bits));
                }
            }
            variants.extend([("cfd", 24), ("fp32", 24)]);
            let mut cbd = Vec::new();
            for (family, bits) in variants {
                let tick = Instant::now();
                let trajectory = match qualification_trajectory(family, bits, &x, &a, c.as_ref(), n)
                {
                    Ok(v) => v,
                    Err(e) => {
                        serde_json::to_writer(
                            &mut rejected,
                            &json!({"input":input.id,"workload":workload,"parameter":param,"family":family,"bits":bits,"reason":e.to_string()}),
                        )?;
                        writeln!(rejected)?;
                        rejected_count += 1;
                        continue;
                    }
                };
                let ns = tick.elapsed().as_nanos();
                let certified = family.starts_with("cbd") || family == "cfd";
                if certified {
                    for (t, (s, r)) in trajectory.iter().zip(&refs).enumerate() {
                        // Disjointness is a definite failure. Reference enclosure need not be a subset of a tighter valid bound.
                        if s.upper < r.lower || s.lower > r.upper {
                            bail!("inclusion failure {family} {} {param} t={t}", input.id);
                        }
                    }
                }
                if input.set == "historical" {
                    for (t, (s, r)) in trajectory.iter().zip(&refs).enumerate() {
                        let center = (&s.lower + &s.upper) / BigInt::from(2);
                        let rc = (&r.lower + &r.upper) / BigInt::from(2);
                        trajectories.write_record([
                            input.id.clone(),
                            workload.clone(),
                            param.clone(),
                            family.into(),
                            bits.to_string(),
                            t.to_string(),
                            center.to_f64().unwrap().to_string(),
                            s.lower.to_f64().unwrap().to_string(),
                            s.upper.to_f64().unwrap().to_string(),
                            rc.to_f64().unwrap().to_string(),
                            (&center - &rc).abs().to_f64().unwrap().to_string(),
                            (&s.upper - &s.lower).to_f64().unwrap().to_string(),
                        ])?;
                    }
                }
                for h in &horizons {
                    for threshold in &thresholds {
                        let first = trajectory
                            .iter()
                            .position(|s| decision(s, threshold).is_none())
                            .filter(|t| *t <= *h);
                        emit(
                            &mut raw,
                            input,
                            workload,
                            param,
                            innovation,
                            family,
                            bits,
                            *h,
                            threshold,
                            &trajectory[*h],
                            &refs[*h],
                            ref_bits,
                            certified,
                            first,
                            vec![bits],
                            vec![ns],
                            ns,
                        )?;
                        rows += 1;
                    }
                }
                if family == "cbd-local" {
                    cbd.push((bits, trajectory));
                }
            }
            if cbd.len() != 4 {
                bail!("missing certified ladder");
            }
            for h in &horizons {
                for threshold in &thresholds {
                    for policy in ["sequential", "margin-directed", "preview-all"] {
                        let states = cbd.iter().map(|(_, v)| v[*h].clone()).collect::<Vec<_>>();
                        // Selection uses only available certified states, never refs. For accounting, recompute every requested candidate independently from x0.
                        let started = Instant::now();
                        let indices = selected(policy, &states, threshold);
                        let mut times = Vec::new();
                        let mut verified = Vec::new();
                        for i in &indices {
                            let tick = Instant::now();
                            let v = qualification_trajectory(
                                "cbd-local",
                                m.precision_ladder[*i],
                                &x,
                                &a,
                                c.as_ref(),
                                *h,
                            )?;
                            times.push(tick.elapsed().as_nanos());
                            verified.push(v[*h].clone());
                        }
                        let chosen = if policy == "preview-all" {
                            verified
                                .iter()
                                .position(|s| decision(s, threshold).is_some())
                                .unwrap_or(verified.len() - 1)
                        } else {
                            verified.len() - 1
                        };
                        let selected_bits = m.precision_ladder[indices[chosen]];
                        emit(
                            &mut raw,
                            input,
                            workload,
                            param,
                            innovation,
                            &format!("cdad-{policy}"),
                            selected_bits,
                            *h,
                            threshold,
                            &verified[chosen],
                            &refs[*h],
                            ref_bits,
                            true,
                            None,
                            indices.iter().map(|i| m.precision_ladder[*i]).collect(),
                            times,
                            started.elapsed().as_nanos(),
                        )?;
                        rows += 1;
                    }
                }
            }
        }
        eprintln!("completed input {}", input.id);
    }
    raw.flush()?;
    rejected.flush()?;
    trajectories.flush()?;
    save(
        &output.join("manifest.json"),
        &json!({"schema_version":1,"status":"numeric-complete","full_matrix":max_inputs.is_none()&&max_horizon.is_none(),"rows":rows,"rejected":rejected_count,"inputs":inputs.len(),"horizons":horizons,"proof_status":"not-executed","timing_campaign_status":"not-executed","matrix_hash":digest(&fs::read(output.join("matrix.json"))?),"inputs_hash":digest(&fs::read(output.join("inputs.json"))?),"raw_hash":digest(&fs::read(output.join("raw.jsonl"))?)}),
    )
}
#[allow(clippy::too_many_arguments)]
fn emit(
    w: &mut impl Write,
    input: &Input,
    workload: &str,
    param: &str,
    innovation: &Option<String>,
    family: &str,
    bits: usize,
    h: usize,
    t: &BigRational,
    s: &QualificationState,
    r: &QualificationState,
    ref_bits: u32,
    certified: bool,
    first: Option<usize>,
    evaluations: Vec<usize>,
    evaluation_ns: Vec<u128>,
    selection_ns: u128,
) -> Result<()> {
    let d = decision(s, t);
    let rd = decision(r, t);
    let flip = d.zip(rd).map(|(d, r)| d != r);
    if certified && flip == Some(true) {
        bail!("incorrect certified decision");
    }
    let profile = json!({"schema":1,"workload":workload,"parameter":param,"innovation":innovation,"family":family,"bits":bits,"threshold":t.to_string(),"operations":"historical-circuit-witness-v1","cfd_corridor":if workload=="affine" {"-24:1"}else{"-16:0"}});
    let center = (&s.lower + &s.upper) / BigInt::from(2);
    let rc = (&r.lower + &r.upper) / BigInt::from(2);
    let row = Row {
        input_id: input.id.clone(),
        input_set: input.set.clone(),
        workload: workload.into(),
        parameter: param.into(),
        innovation: innovation.clone(),
        family: family.into(),
        precision: bits,
        horizon: h,
        threshold: t.to_string(),
        profile_hash: digest(&serde_json::to_vec(&profile)?),
        lower: s.lower.to_string(),
        upper: s.upper.to_string(),
        reference_lower: r.lower.to_string(),
        reference_upper: r.upper.to_string(),
        reference_bits: ref_bits,
        decision: label(d),
        reference_decision: rd,
        predicate_flip: flip,
        certified,
        first_abstention: first,
        center_absolute_error: (center - rc).abs().to_f64().unwrap_or(f64::MAX),
        enclosure_width: (&s.upper - &s.lower).to_f64().unwrap_or(f64::MAX),
        evaluations,
        evaluation_ns,
        selection_ns,
        proof_attempts: None,
        proof_cost_ns: None,
        constraints: None,
    };
    serde_json::to_writer(&mut *w, &row)?;
    writeln!(w)?;
    Ok(())
}
fn select_public(
    x0: &str,
    r: &str,
    threshold: &str,
    steps: usize,
    policy: &str,
    precision: usize,
) -> Result<()> {
    if !["fixed", "sequential", "margin-directed", "preview-all"].contains(&policy)
        || ![16, 32, 48, 64].contains(&precision)
        || steps == 0
        || steps > 1000
    {
        bail!("invalid selection profile");
    }
    let x = q(x0)?;
    let r = q(r)?;
    let threshold = q(threshold)?;
    if x < BigRational::from_integer(0.into())
        || x > BigRational::one()
        || r < BigRational::from_integer(0.into())
        || r > BigRational::from_integer(4.into())
    {
        bail!("domain");
    }
    let start = Instant::now();
    let mut attempts = Vec::new();
    let mut index = if policy == "fixed" {
        precision / 16 - 1
    } else {
        0
    };
    loop {
        let bits = 16 * (index + 1);
        let tick = Instant::now();
        let v = qualification_trajectory("cbd-local", bits, &x, &r, None, steps)?;
        let s = &v[steps];
        let d = decision(s, &threshold);
        attempts.push(json!({"precision":bits,"decision":label(d),"lower":s.lower.to_string(),"upper":s.upper.to_string(),"evaluation_ns":tick.elapsed().as_nanos()}));
        if policy == "fixed" || index == 3 || (policy != "preview-all" && d.is_some()) {
            break;
        }
        let mut next = index + 1;
        if policy == "margin-directed" {
            let margin = ((&s.lower + &s.upper) / BigInt::from(2) - &threshold).abs();
            let radius = (&s.upper - &s.lower) / BigInt::from(2);
            while next < 3 && &radius / BigInt::from(1u64 << (16 * (next - index))) >= margin {
                next += 1;
            }
        }
        index = next;
    }
    let chosen = if policy == "preview-all" {
        attempts
            .iter()
            .position(|v| v["decision"] != "Indeterminate")
            .unwrap_or(attempts.len() - 1)
    } else {
        attempts.len() - 1
    };
    println!(
        "{}",
        json!({"schema_version":1,"policy":policy,"selected_precision":attempts[chosen]["precision"],"decision":attempts[chosen]["decision"],"attempts":attempts,"selection_ns":start.elapsed().as_nanos()})
    );
    Ok(())
}
fn main() -> Result<()> {
    match Args::parse().command {
        Command::Select {
            x0,
            r,
            threshold,
            steps,
            policy,
            precision,
        } => select_public(&x0, &r, &threshold, steps, &policy, precision),
        Command::Generate { matrix, output_dir } => generate(&matrix, &output_dir),
        Command::Evaluate {
            inputs_dir,
            output_dir,
            max_inputs,
            max_horizon,
        } => evaluate(&inputs_dir, &output_dir, max_inputs, max_horizon),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generator_vector() {
        let mut state = 0;
        assert_eq!(splitmix(&mut state), 0xe220a8397b1dcdaf);
    }
    #[test]
    fn boundaries() {
        let s = QualificationState {
            lower: q("1/2").unwrap(),
            upper: q("3/4").unwrap(),
        };
        assert_eq!(decision(&s, &q("1/2").unwrap()), Some(true));
        assert_eq!(decision(&s, &q("3/4").unwrap()), None);
    }
    #[test]
    fn all_previews() {
        let s = QualificationState {
            lower: q("1").unwrap(),
            upper: q("1").unwrap(),
        };
        assert_eq!(
            selected("preview-all", &vec![s; 4], &q("1/2").unwrap()),
            vec![0, 1, 2, 3]
        );
    }
}

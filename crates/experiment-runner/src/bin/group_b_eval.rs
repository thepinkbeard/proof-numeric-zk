//! Controlled pilot, NOT a proving benchmark or a reproduction of an external backend.
use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use num_bigint::BigInt;
use num_rational::BigRational as Q;
use num_traits::{One, Signed, Zero};
use numeric_approximations::{
    evaluate_pade, evaluate_taylor, generate_sigmoid_certificate, prepare_sigmoid_contract,
    OperationCounts,
};
use numeric_core::{
    select_checked_precision, BoundBreakdown, CheckedEnclosure, ContractPrecisionPolicy, Decision3,
    FixedPointFormat, FunctionId, OverflowMode, QuantizationRounding, SigmoidCertificate,
    SigmoidRelationContract,
};
use numeric_reference::evaluate_transcendental;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
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
    Generate {
        #[arg(long)]
        matrix: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
    },
    Verify {
        #[arg(long)]
        matrix: PathBuf,
        #[arg(long)]
        run_dir: PathBuf,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Matrix {
    schema_version: u16,
    run_kind: String,
    fractional_bits: Vec<u16>,
    gl_orders: Vec<usize>,
    taylor_degrees: Vec<usize>,
    pade_orders: Vec<usize>,
    integer_bits: u16,
    rounding: String,
    generator: String,
    seeds: Vec<u64>,
    vectors_per_seed: usize,
    grid_intervals: usize,
    thresholds: Vec<String>,
    reference_bits: u32,
    reference_max_bits: u32,
    adversarial_offset_bits: usize,
    search_output_ulps: i64,
    amplification_depths: Vec<u32>,
    relation_version: String,
    attribution: String,
    timing_claim: bool,
    proof_backend: Option<String>,
}
impl Matrix {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.run_kind == "controlled-off-circuit-pilot",
            "unsupported schema"
        );
        ensure!(
            self.fractional_bits == [16, 32, 48, 64] && self.gl_orders == [2, 4, 8, 16],
            "unsupported ladder"
        );
        ensure!(
            self.taylor_degrees == [3, 5, 7, 9, 13] && self.pade_orders == [2, 3, 4, 5, 8],
            "unsupported methods"
        );
        ensure!(
            self.integer_bits == 8 && self.rounding == "nearest-ties-to-even",
            "unsupported format"
        );
        ensure!(
            self.generator == "splitmix64-v1" && !self.seeds.is_empty() && self.seeds.len() <= 10,
            "invalid seeds"
        );
        ensure!(
            self.seeds.iter().collect::<BTreeSet<_>>().len() == self.seeds.len(),
            "duplicate seed"
        );
        ensure!(
            (1..=1000).contains(&self.vectors_per_seed)
                && (4..=1024).contains(&self.grid_intervals),
            "invalid dataset size"
        );
        ensure!(
            self.thresholds == ["2/5", "1/2", "3/5"],
            "unsupported thresholds"
        );
        ensure!(
            self.reference_bits == 512 && self.reference_max_bits == 16384,
            "unsupported reference"
        );
        ensure!(
            self.adversarial_offset_bits == 24 && self.search_output_ulps == 32,
            "unsupported adversarial protocol"
        );
        ensure!(
            self.amplification_depths == [0, 1, 2, 4, 8],
            "unsupported depths"
        );
        ensure!(
            self.relation_version == "sigmoid-certificate-v1"
                && self.attribution == "our-extension-of-implicit-quadrature-relation",
            "unsupported contract"
        );
        ensure!(
            !self.timing_claim && self.proof_backend.is_none(),
            "pilot cannot claim proof cost"
        );
        Ok(())
    }
    fn format(&self, bits: u16) -> FixedPointFormat {
        FixedPointFormat {
            integer_bits: self.integer_bits,
            fractional_bits: bits,
            signed: true,
            rounding: QuantizationRounding::NearestTiesToEven,
            overflow: OverflowMode::Reject,
        }
    }
}
fn q(n: i64, d: i64) -> Q {
    Q::new(n.into(), d.into())
}
fn thresholds() -> [Q; 3] {
    [q(2, 5), q(1, 2), q(3, 5)]
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn load<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Sample {
    id: String,
    cohort: String,
    seed: Option<u64>,
    inputs: [Q; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Inputs {
    scalar_grid: Vec<Q>,
    samples: Vec<Sample>,
}
fn inputs(m: &Matrix) -> Inputs {
    let mut samples = Vec::new();
    for &seed in &m.seeds {
        let mut state = seed;
        for i in 0..m.vectors_per_seed {
            let xs = std::array::from_fn(|_| {
                Q::new(BigInt::from(next(&mut state)), BigInt::one() << 63) - Q::one()
            });
            samples.push(Sample {
                id: format!("seed-{seed}-{i}"),
                cohort: "random-pilot".into(),
                seed: Some(seed),
                inputs: xs,
            });
        }
    }
    let pairs = [q(-3, 4), q(-1, 4), q(1, 4), q(3, 4)];
    let epsilon = Q::new(BigInt::one(), BigInt::one() << m.adversarial_offset_bits);
    let mut plus = pairs.clone();
    plus[0] += &epsilon;
    let mut minus = pairs.clone();
    minus[0] -= &epsilon;
    for (id, xs) in [
        ("all-minus-one", std::array::from_fn(|_| -Q::one())),
        ("all-plus-one", std::array::from_fn(|_| Q::one())),
        ("symmetric-boundary", pairs),
        ("near-boundary-plus", plus),
        ("near-boundary-minus", minus),
    ] {
        samples.push(Sample {
            id: id.into(),
            cohort: "adversarial".into(),
            seed: None,
            inputs: xs,
        });
    }
    Inputs {
        scalar_grid: (0..=m.grid_intervals)
            .map(|i| q(2 * i as i64, m.grid_intervals as i64) - Q::one())
            .collect(),
        samples,
    }
}
fn scalar_inputs(data: &Inputs) -> BTreeSet<Q> {
    data.scalar_grid
        .iter()
        .cloned()
        .chain(data.samples.iter().flat_map(|s| s.inputs.clone()))
        .collect()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Reference {
    lower: Q,
    upper: Q,
    bits: u32,
}
fn reference(xs: &[Q], m: &Matrix) -> Result<Reference> {
    let mut bits = m.reference_bits;
    loop {
        let mut lower = Q::zero();
        let mut upper = Q::zero();
        for x in xs {
            let r = evaluate_transcendental(FunctionId::Sigmoid, x, bits)?;
            lower += r.lower_rational();
            upper += r.upper_rational();
        }
        lower /= BigInt::from(xs.len());
        upper /= BigInt::from(xs.len());
        let r = Reference { lower, upper, bits };
        if thresholds().iter().all(|t| ref_decision(&r, t).is_some())
            || bits == m.reference_max_bits
        {
            return Ok(r);
        }
        bits = (bits * 2).min(m.reference_max_bits);
    }
}
fn ref_decision(r: &Reference, t: &Q) -> Option<bool> {
    if &r.lower >= t {
        Some(true)
    } else if &r.upper < t {
        Some(false)
    } else {
        None
    }
}
fn error_interval(y: &Q, r: &Reference) -> (Q, Q) {
    let a = (y - &r.lower).abs();
    let b = (y - &r.upper).abs();
    let lower = if y >= &r.lower && y <= &r.upper {
        Q::zero()
    } else {
        a.clone().min(b.clone())
    };
    (lower, a.max(b))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct WitnessRow {
    bits: u16,
    order: usize,
    certificate: SigmoidCertificate,
    bound: BoundBreakdown,
}
type Cache = BTreeMap<(u16, usize, Q), (CheckedEnclosure, Q)>;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct DecisionRow {
    sample_id: String,
    cohort: String,
    bits: u16,
    order: usize,
    threshold: Q,
    output: Q,
    lower: Q,
    upper: Q,
    reference: Reference,
    reference_decision: Option<bool>,
    decision: Decision3,
    observed_flip: Option<bool>,
    error_lower: Q,
    error_upper: Q,
}
fn decisions(m: &Matrix, data: &Inputs, cache: &Cache) -> Result<Vec<DecisionRow>> {
    let mut rows = Vec::new();
    for s in &data.samples {
        let reference = reference(&s.inputs, m)?;
        for &bits in &m.fractional_bits {
            for &order in &m.gl_orders {
                let mut enclosure = CheckedEnclosure::exact(Q::zero());
                let mut output = Q::zero();
                for x in &s.inputs {
                    let (e, y) = cache
                        .get(&(bits, order, x.clone()))
                        .context("missing scalar certificate")?;
                    enclosure = enclosure.add(e);
                    output += y;
                }
                enclosure = enclosure.scale(&q(1, 4));
                output /= BigInt::from(4);
                ensure!(
                    enclosure.contains(&reference.lower) && enclosure.contains(&reference.upper),
                    "reference escapes checked composition"
                );
                for threshold in thresholds() {
                    let decision = enclosure.decision(&threshold);
                    let rd = ref_decision(&reference, &threshold);
                    if let Some(rd) = rd {
                        ensure!(
                            decision == Decision3::Indeterminate
                                || (decision == Decision3::True) == rd,
                            "certified contradiction"
                        );
                    }
                    let (el, eu) = error_interval(&output, &reference);
                    rows.push(DecisionRow {
                        sample_id: s.id.clone(),
                        cohort: s.cohort.clone(),
                        bits,
                        order,
                        observed_flip: rd.map(|d| d != (output >= threshold)),
                        threshold,
                        output: output.clone(),
                        lower: enclosure.lower().clone(),
                        upper: enclosure.upper().clone(),
                        reference: reference.clone(),
                        reference_decision: rd,
                        decision,
                        error_lower: el,
                        error_upper: eu,
                    });
                }
            }
        }
    }
    Ok(rows)
}
fn policies(m: &Matrix, data: &Inputs, cache: &Cache) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    for s in &data.samples {
        for &order in &m.gl_orders {
            for threshold in thresholds() {
                for (policy, fixed) in [
                    (ContractPrecisionPolicy::Fixed, 16),
                    (ContractPrecisionPolicy::Fixed, 32),
                    (ContractPrecisionPolicy::Fixed, 48),
                    (ContractPrecisionPolicy::Fixed, 64),
                    (ContractPrecisionPolicy::Sequential, 16),
                    (ContractPrecisionPolicy::MarginDirected, 16),
                    (ContractPrecisionPolicy::PreviewAll, 16),
                ] {
                    let choice = select_checked_precision(policy, fixed, &threshold, |bits| {
                        let e = s
                            .inputs
                            .iter()
                            .fold(CheckedEnclosure::exact(Q::zero()), |e, x| {
                                e.add(&cache[&(bits, order, x.clone())].0)
                            });
                        Ok(e.scale(&q(1, 4)))
                    })?;
                    rows.push(json!({"sample_id":s.id,"cohort":s.cohort,"order":order,"threshold":threshold,
                "policy":policy,"fixed_bits":fixed,"selected_bits":choice.selected_precision,
                "decision":choice.enclosure.decision(&threshold),"mode":"logical-replay-no-cost-claim",
                "evaluated_bits":choice.evaluations.iter().map(|e|e.precision_bits).collect::<Vec<_>>(),
                "runtime_ns":null,"proof_count":0,"constraints":null}));
                }
            }
        }
    }
    Ok(rows)
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct PrimitiveRow {
    bits: u16,
    method: String,
    order: usize,
    input: Q,
    output: Q,
    reference: Reference,
    error_lower: Q,
    error_upper: Q,
    counts: OperationCounts,
}
fn primitives(m: &Matrix, data: &Inputs) -> Result<Vec<PrimitiveRow>> {
    let mut rows = Vec::new();
    for x in &data.scalar_grid {
        let reference = reference(std::slice::from_ref(x), m)?;
        for &bits in &m.fractional_bits {
            for (method, orders) in [("taylor", &m.taylor_degrees), ("pade", &m.pade_orders)] {
                for &order in orders {
                    let v = if method == "taylor" {
                        evaluate_taylor(FunctionId::Sigmoid, order, x, &m.format(bits))?
                    } else {
                        evaluate_pade(FunctionId::Sigmoid, order, x, &m.format(bits))?
                    };
                    let (el, eu) = error_interval(&v.fixed_value, &reference);
                    let mut counts = v.counts;
                    // Legacy metadata describes a paper relation, not this measured implementation.
                    counts.verification_relation_depth = None;
                    rows.push(PrimitiveRow {
                        bits,
                        method: method.into(),
                        order,
                        input: x.clone(),
                        output: v.fixed_value,
                        reference: reference.clone(),
                        error_lower: el,
                        error_upper: eu,
                        counts,
                    });
                }
            }
        }
    }
    Ok(rows)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SearchRow {
    input: Q,
    honest: SigmoidCertificate,
    accepted: Vec<SigmoidCertificate>,
    rejected: usize,
}
fn search_candidate(
    c: &SigmoidRelationContract,
    honest: &SigmoidCertificate,
    offset: i64,
) -> Result<SigmoidCertificate> {
    let output = &honest.output + c.format.ulp() * BigInt::from(offset);
    let a = &output * BigInt::from(2) - Q::one();
    let terms = c
        .nodes
        .iter()
        .map(|r| {
            c.format
                .encode(&(&a / (Q::one() + &a * r)))
                .map(|x| x.value.decode())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SigmoidCertificate {
        output,
        terms,
        ..honest.clone()
    })
}
fn searches(
    m: &Matrix,
    c: &SigmoidRelationContract,
    witnesses: &[WitnessRow],
) -> Result<Vec<SearchRow>> {
    let mut rows = Vec::new();
    for input in [q(-3, 4), q(0, 1), q(3, 4)] {
        let honest = witnesses
            .iter()
            .find(|w| w.bits == 16 && w.order == 4 && w.certificate.canonical_input == input)
            .context("missing search anchor")?
            .certificate
            .clone();
        let mut accepted = Vec::new();
        let mut rejected = 0;
        for offset in -m.search_output_ulps..=m.search_output_ulps {
            let candidate = search_candidate(c, &honest, offset)?;
            if c.check(&input, &candidate).is_ok() {
                accepted.push(candidate);
            } else {
                rejected += 1;
            }
        }
        rows.push(SearchRow {
            input,
            honest,
            accepted,
            rejected,
        });
    }
    Ok(rows)
}
fn checked_cache(
    m: &Matrix,
    data: &Inputs,
    contracts: &[SigmoidRelationContract],
    witnesses: &[WitnessRow],
) -> Result<Cache> {
    ensure!(
        contracts.len() == m.fractional_bits.len() * m.gl_orders.len(),
        "contract count mismatch"
    );
    let mut by_key = BTreeMap::new();
    for c in contracts {
        c.validate()?;
        let key = (c.format.fractional_bits, c.nodes.len());
        ensure!(
            m.fractional_bits.contains(&key.0) && m.gl_orders.contains(&key.1),
            "unauthorized profile"
        );
        ensure!(
            c.format == m.format(key.0)
                && c.moment_degree == (2 * key.0 + 8).min(128)
                && c.term_slack == c.format.ulp() * BigInt::from(2)
                && c.sum_slack == c.format.ulp() * BigInt::from(16),
            "unauthorized contract parameters"
        );
        ensure!(by_key.insert(key, c).is_none(), "duplicate contract");
    }
    let xs = scalar_inputs(data);
    ensure!(
        witnesses.len() == xs.len() * by_key.len(),
        "witness count mismatch"
    );
    let mut cache = BTreeMap::new();
    for w in witnesses {
        let c = by_key.get(&(w.bits, w.order)).context("unknown profile")?;
        let x = &w.certificate.canonical_input;
        ensure!(xs.contains(x), "unknown input");
        let (e, b) = c.check(x, &w.certificate)?;
        ensure!(
            serde_json::to_value(&b)? == serde_json::to_value(&w.bound)?,
            "bound mismatch"
        );
        ensure!(
            cache
                .insert(
                    (w.bits, w.order, x.clone()),
                    (e, w.certificate.output.clone())
                )
                .is_none(),
            "duplicate witness"
        );
    }
    Ok(cache)
}
fn command(program: &str, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new(program).args(args).output()?;
    ensure!(out.status.success(), "metadata command failed");
    Ok(String::from_utf8(out.stdout)?.trim().into())
}
fn generate(matrix: &Path, dir: &Path) -> Result<()> {
    let m: Matrix = load(matrix)?;
    m.validate()?;
    fs::create_dir(dir)?; // Reserve atomically: no overwrite and no success marker on failure.
    save(&dir.join("matrix.json"), &m)?;
    let data = inputs(&m);
    save(&dir.join("inputs.json"), &data)?;
    let mut sources = BTreeMap::new();
    let names = command(
        "git",
        &["ls-files", "--cached", "--others", "--exclude-standard"],
    )?;
    for name in names.lines() {
        if name.starts_with("crates/")
            || name.starts_with("experiments/")
            || name.starts_with("scripts/")
            || name == "Cargo.lock"
            || name == "Cargo.toml"
        {
            sources.insert(name.to_string(), hash(&fs::read(name)?));
        }
    }
    save(
        &dir.join("provenance.json"),
        &json!({"matrix_sha256":hash(&fs::read(matrix)?),
        "commit":command("git",&["rev-parse","HEAD"])?,"git_status":command("git",&["status","--porcelain"])?,
        "git_diff_sha256":hash(command("git",&["diff","HEAD","--binary"])?.as_bytes()),
        "rustc":command("rustc",&["--version"])?,"system":command("uname",&["-a"])?,
        "sources":sources,"evidence":"our-analytical-checker-plus-observed-pilot","proof_backend":null}),
    )?;
    let mut timings = Vec::new();
    let mut contracts = Vec::new();
    let mut witnesses = Vec::new();
    let xs = scalar_inputs(&data);
    let started = Instant::now();
    for &bits in &m.fractional_bits {
        for &order in &m.gl_orders {
            let tick = Instant::now();
            let c = prepare_sigmoid_contract(m.format(bits), order)?;
            timings.push(json!({"stage":"prepare","bits":bits,"order":order,"elapsed_ns":tick.elapsed().as_nanos()}));
            eprintln!("GL F{bits} order {order}: {} scalar inputs", xs.len());
            for x in &xs {
                let tick = Instant::now();
                let certificate = generate_sigmoid_certificate(&c, x)?;
                let generation_ns = tick.elapsed().as_nanos();
                let tick = Instant::now();
                let (_, bound) = c.check(x, &certificate)?;
                timings.push(json!({"stage":"scalar","bits":bits,"order":order,"input":x,
                "generation_including_internal_checks_ns":generation_ns,"additional_check_ns":tick.elapsed().as_nanos()}));
                witnesses.push(WitnessRow {
                    bits,
                    order,
                    certificate,
                    bound,
                });
            }
            contracts.push(c);
            // Diagnostic checkpoints survive a failure without a completion manifest.
            save(
                &dir.join(format!("checkpoint-f{bits}-n{order}.json")),
                &json!({"elapsed_ns":started.elapsed().as_nanos()}),
            )?;
        }
    }
    save(&dir.join("contracts.json"), &contracts)?;
    save(&dir.join("witnesses.json"), &witnesses)?;
    let cache = checked_cache(&m, &data, &contracts, &witnesses)?;
    save(&dir.join("decisions.json"), &decisions(&m, &data, &cache)?)?;
    save(&dir.join("policies.json"), &policies(&m, &data, &cache)?)?;
    save(&dir.join("primitives.json"), &primitives(&m, &data)?)?;
    let c = contracts
        .iter()
        .find(|c| c.format.fractional_bits == 16 && c.nodes.len() == 4)
        .unwrap();
    save(&dir.join("search.json"), &searches(&m, c, &witnesses)?)?;
    save(&dir.join("timings.json"), &timings)?;
    let mut files = BTreeMap::new();
    for name in [
        "matrix.json",
        "inputs.json",
        "provenance.json",
        "contracts.json",
        "witnesses.json",
        "decisions.json",
        "policies.json",
        "primitives.json",
        "search.json",
        "timings.json",
    ] {
        files.insert(name, hash(&fs::read(dir.join(name))?));
    }
    save(
        &dir.join("manifest.json"),
        &json!({"schema_version":1,"status":"complete-pilot",
        "files":files,"elapsed_ns":started.elapsed().as_nanos(),"proof_count":0,"constraints":null,
        "timing_claim":false,"scalar_witnesses":witnesses.len(),"sample_vectors":data.samples.len()}),
    )?;
    Ok(())
}
fn verify(matrix: &Path, dir: &Path) -> Result<()> {
    let m: Matrix = load(matrix)?;
    m.validate()?;
    ensure!(
        load::<Matrix>(&dir.join("matrix.json"))? == m,
        "matrix mismatch"
    );
    let manifest: Value = load(&dir.join("manifest.json"))?;
    ensure!(
        manifest["status"] == "complete-pilot"
            && manifest["proof_count"] == 0
            && manifest["timing_claim"] == false,
        "invalid manifest"
    );
    for name in [
        "matrix.json",
        "inputs.json",
        "provenance.json",
        "contracts.json",
        "witnesses.json",
        "decisions.json",
        "policies.json",
        "primitives.json",
        "search.json",
        "timings.json",
    ] {
        ensure!(
            manifest["files"][name] == hash(&fs::read(dir.join(name))?),
            "artifact digest mismatch: {name}"
        );
    }
    let data: Inputs = load(&dir.join("inputs.json"))?;
    ensure!(data == inputs(&m), "input generation mismatch");
    let contracts: Vec<SigmoidRelationContract> = load(&dir.join("contracts.json"))?;
    // A valid moment bound need not use Legendre nodes. Check their identity
    // separately so an arbitrary safe rule cannot masquerade as the frozen GL
    // benchmark. MPFR regenerates constants here, never authorizes the radius.
    ensure!(contracts.len() == 16, "contract count mismatch");
    for c in &contracts {
        let bits = c.format.fractional_bits;
        let order = c.nodes.len();
        ensure!(
            m.fractional_bits.contains(&bits) && m.gl_orders.contains(&order),
            "unauthorized profile"
        );
        let expected = prepare_sigmoid_contract(m.format(bits), order)?;
        ensure!(
            expected.hash_hex() == c.hash_hex(),
            "frozen GL constants mismatch"
        );
    }
    let witnesses: Vec<WitnessRow> = load(&dir.join("witnesses.json"))?;
    let cache = checked_cache(&m, &data, &contracts, &witnesses)?;
    ensure!(
        decisions(&m, &data, &cache)? == load::<Vec<DecisionRow>>(&dir.join("decisions.json"))?,
        "composition/reference mismatch"
    );
    ensure!(
        policies(&m, &data, &cache)? == load::<Vec<Value>>(&dir.join("policies.json"))?,
        "policy mismatch"
    );
    ensure!(
        primitives(&m, &data)? == load::<Vec<PrimitiveRow>>(&dir.join("primitives.json"))?,
        "primitive mismatch"
    );
    let c = contracts
        .iter()
        .find(|c| c.format.fractional_bits == 16 && c.nodes.len() == 4)
        .unwrap();
    ensure!(
        serde_json::to_value(searches(&m, c, &witnesses)?)?
            == load::<Value>(&dir.join("search.json"))?,
        "accepted witness search mismatch"
    );
    println!(
        "Verified {} scalar certificates and {} vectors; off-circuit only.",
        witnesses.len(),
        data.samples.len()
    );
    Ok(())
}
fn main() -> Result<()> {
    match Args::parse().command {
        Command::Generate { matrix, output_dir } => generate(&matrix, &output_dir),
        Command::Verify { matrix, run_dir } => verify(&matrix, &run_dir),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn matrix() -> Matrix {
        serde_json::from_str(include_str!(
            "../../../../experiments/qualification_group_b_pilot_v1.json"
        ))
        .unwrap()
    }
    #[test]
    fn frozen_generator_and_dataset() {
        let m = matrix();
        m.validate().unwrap();
        let mut state = 0;
        assert_eq!(next(&mut state), 0xe220a8397b1dcdaf);
        let a = inputs(&m);
        assert_eq!(a, inputs(&m));
        assert_eq!(a.samples.len(), 13);
        assert_eq!(
            a.samples
                .iter()
                .filter(|s| s.cohort == "random-pilot")
                .count(),
            8
        );
        assert!(scalar_inputs(&a).iter().all(|x| x.abs() <= Q::one()));
        let mut invalid = m;
        invalid.timing_claim = true;
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn equality_is_not_decided_from_reference_center() {
        let r = Reference {
            lower: q(49, 100),
            upper: q(51, 100),
            bits: 64,
        };
        assert_eq!(ref_decision(&r, &q(1, 2)), None);
        assert_eq!(error_interval(&q(1, 2), &r), (q(0, 1), q(1, 100)));
        assert_eq!(
            ref_decision(
                &Reference {
                    lower: q(1, 2),
                    upper: q(1, 2),
                    bits: 64
                },
                &q(1, 2)
            ),
            Some(true)
        );
    }
    #[test]
    fn output_perturbation_still_requires_full_checker() {
        let m = matrix();
        let c = prepare_sigmoid_contract(m.format(16), 4).unwrap();
        let h = generate_sigmoid_certificate(&c, &q(0, 1)).unwrap();
        assert!(c
            .check(&q(0, 1), &search_candidate(&c, &h, 0).unwrap())
            .is_ok());
        assert!(c
            .check(&q(0, 1), &search_candidate(&c, &h, 32).unwrap())
            .is_err());
        let mut bad = h.clone();
        bad.claimed_radius = Q::zero();
        assert!(c.check(&q(0, 1), &bad).is_err());
        bad = h;
        bad.canonical_input = q(1, 2);
        assert!(c.check(&q(0, 1), &bad).is_err());
    }
    #[test]
    fn exact_linear_composition_scales_all_accepted_errors() {
        let m = matrix();
        let c = prepare_sigmoid_contract(m.format(16), 4).unwrap();
        let h = generate_sigmoid_certificate(&c, &q(0, 1)).unwrap();
        for offset in -32..=32 {
            let candidate = search_candidate(&c, &h, offset).unwrap();
            if let Ok((enclosure, _)) = c.check(&q(0, 1), &candidate) {
                for k in &m.amplification_depths {
                    let gain = Q::from_integer(BigInt::one() << *k as usize);
                    assert!(enclosure.scale(&gain).contains(&(gain * q(1, 2))));
                }
            }
        }
    }
}

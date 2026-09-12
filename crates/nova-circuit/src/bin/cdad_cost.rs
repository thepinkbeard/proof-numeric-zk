//! Measures every fixed-precision circuit used by the adaptive CDAD policy.

use anyhow::Result;
use clap::Parser;
use nova_circuit::{
    half_threshold_numerator, initial_cbd_state, CbdLogisticStepCircuit, CbdRadiusBound,
    CbdStepWitness, CdadFixedLogisticStepCircuit, CdadLinearScoreCircuit, CdadLinearScoreWitness,
    SignedCdadValue,
};
use nova_snark::{
    frontend::{num::AllocatedNum, test_cs::TestConstraintSystem, ConstraintSystem},
    provider::PallasEngine,
    traits::{circuit::StepCircuit, Engine},
};
use num_rational::BigRational;
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::PathBuf};

type Scalar = <PallasEngine as Engine>::Scalar;

#[derive(Parser, Debug)]
#[command(about = "Mede os circuitos fixos que compõem uma execução CDAD")]
struct Args {
    #[arg(long, default_value = "results/cdad_precision_cost.json")]
    output: PathBuf,
}

#[derive(Serialize)]
struct PrecisionCost {
    precision_bits: usize,
    raw_step_constraints: usize,
    state_field_elements: usize,
    selection_constraints: usize,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    circuit: &'static str,
    curve: &'static str,
    precision_ladder: [usize; 4],
    costs: BTreeMap<usize, PrecisionCost>,
    score_costs: BTreeMap<String, BTreeMap<usize, ScoreCost>>,
}

#[derive(Serialize)]
struct ScoreCost {
    precision_bits: usize,
    terms: usize,
    raw_step_constraints: usize,
    state_field_elements: usize,
}

fn measure<const K: usize>() -> PrecisionCost {
    let x0 = BigRational::new(1.into(), 10.into());
    let (limbs, is_one, radius) = initial_cbd_state::<K>(&x0);
    let witness = CbdStepWitness::compute_with_threshold(
        limbs,
        is_one,
        radius,
        37,
        10,
        CbdRadiusBound::LocalTaylor,
        half_threshold_numerator(K),
    );
    let inner = CbdLogisticStepCircuit {
        witness,
        r_num: 37,
        r_den: 10,
        radius_bound: CbdRadiusBound::LocalTaylor,
        threshold_numerator: half_threshold_numerator(K),
    };
    let circuit = CdadFixedLogisticStepCircuit::new(inner.clone());
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let mut raw_values = vec![0_u64];
    raw_values.extend(limbs);
    raw_values.push(u64::from(is_one));
    raw_values.push(radius as u64);
    raw_values.push(0);
    raw_values.push((16 * K) as u64);
    let z = raw_values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cs.namespace(|| format!("z_{index}")), || {
                Ok(Scalar::from(*value))
            })
            .expect("estado alocado")
        })
        .collect::<Vec<_>>();
    circuit.synthesize(&mut cs, &z).expect("síntese CDAD");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    let total = cs.num_constraints();

    let mut cbd = TestConstraintSystem::<Scalar>::new();
    let cbd_z = raw_values[..raw_values.len() - 1]
        .iter()
        .copied()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cbd.namespace(|| format!("cbd_z_{index}")), || {
                Ok(Scalar::from(value))
            })
            .expect("estado CBD alocado")
        })
        .collect::<Vec<_>>();
    inner.synthesize(&mut cbd, &cbd_z).expect("síntese CBD");

    PrecisionCost {
        precision_bits: 16 * K,
        raw_step_constraints: total,
        state_field_elements: CdadFixedLogisticStepCircuit::<K>::arity_value(),
        selection_constraints: total.saturating_sub(cbd.num_constraints()),
    }
}

fn measure_score(precision_bits: usize, terms: usize) -> ScoreCost {
    let scale = 1_u128 << precision_bits;
    let value = SignedCdadValue {
        negative: false,
        magnitude: scale / 2,
        radius: 1,
    };
    let witness = CdadLinearScoreWitness::compute(
        precision_bits,
        SignedCdadValue {
            negative: false,
            magnitude: 0,
            radius: 0,
        },
        vec![(value.clone(), value); terms],
        BigRational::new(1.into(), 2.into()),
    );
    let circuit = CdadLinearScoreCircuit { witness };
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let state = [0_u64, 2, precision_bits as u64]
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cs.namespace(|| format!("score_z_{index}")), || {
                Ok(Scalar::from(value))
            })
            .expect("estado de score alocado")
        })
        .collect::<Vec<_>>();
    circuit
        .synthesize(&mut cs, &state)
        .expect("síntese do score CDAD");
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    ScoreCost {
        precision_bits,
        terms,
        raw_step_constraints: cs.num_constraints(),
        state_field_elements: CdadLinearScoreCircuit::arity_value(),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let points = [
        measure::<1>(),
        measure::<2>(),
        measure::<3>(),
        measure::<4>(),
    ];
    let costs = points
        .into_iter()
        .map(|point| (point.precision_bits, point))
        .collect();
    let score_costs = [("ecef_3_terms", 3_usize), ("adult_6_terms", 6_usize)]
        .into_iter()
        .map(|(name, terms)| {
            let per_precision = [16_usize, 32, 48, 64]
                .into_iter()
                .map(|precision| (precision, measure_score(precision, terms)))
                .collect();
            (name.to_owned(), per_precision)
        })
        .collect();
    let report = Report {
        schema_version: 1,
        circuit: "cdad-fixed-local-taylor-logistic",
        curve: "pallas",
        precision_ladder: [16, 32, 48, 64],
        costs,
        score_costs,
    };
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let output = args.output;
    fs::write(&output, serde_json::to_string_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    println!("gravado em {}", output.display());
    Ok(())
}

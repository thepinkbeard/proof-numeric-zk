use ff::Field;
use nova_circuit::{
    circuit_decision, common_threshold, half_threshold_numerator, CarryLogisticStepCircuit2,
    CbdLogisticStepCircuit, CbdRadiusBound, CbdSequentialLogisticStepCircuit,
    CbdSequentialStepWitness, CbdStepWitness, CdadFixedLogisticStepCircuit, CdadLinearScoreCircuit,
    CdadLinearScoreWitness, CfdLogisticStepCircuit, CfdLogisticStepWitness, CfdStableStepCircuit,
    CfdStableStepWitness, CfdTopology, CircuitRounding, CompensatedLogisticStepCircuit,
    CompensatedStepWitness, LogisticStepCircuit, ProductCarryWitness2, SignedCdadValue,
    StepWitness,
};
use nova_snark::{
    frontend::{num::AllocatedNum, test_cs::TestConstraintSystem, ConstraintSystem},
    provider::PallasEngine,
    traits::{circuit::StepCircuit, Engine},
};
use num_rational::BigRational;
use num_traits::ToPrimitive;
use numeric_core::{CertifiedDyadic, CertifiedFloat, CfdProfile, Decision3, NumericProfile};

type Scalar = <PallasEngine as Engine>::Scalar;

/// Limiar `1/2` na grade fina de 64 bits da representação compensada.
const COMPENSATED_HALF: u128 = 1_u128 << 63;

fn comp_fine_numerator(limbs: [u64; 2], is_one: bool, residual_offset: u64) -> u128 {
    let scale = 1_u128 << 32;
    let main = if is_one {
        scale
    } else {
        ((limbs[0] as u128) << 16) + limbs[1] as u128
    };
    (main * scale + residual_offset as u128) - scale / 2
}

fn synthesize(
    input_limbs: [u64; 2],
    input_is_one: bool,
    witness: StepWitness<2>,
    rounding: CircuitRounding,
) -> TestConstraintSystem<Scalar> {
    let mut cs = TestConstraintSystem::new();
    let mut z =
        vec![AllocatedNum::alloc(cs.namespace(|| "iteration"), || Ok(Scalar::ZERO)).unwrap()];
    for (index, limb) in input_limbs.into_iter().enumerate() {
        z.push(
            AllocatedNum::alloc(cs.namespace(|| format!("input_{index}")), || {
                Ok(Scalar::from(limb))
            })
            .unwrap(),
        );
    }
    z.push(
        AllocatedNum::alloc(cs.namespace(|| "input_is_one"), || {
            Ok(Scalar::from(input_is_one as u64))
        })
        .unwrap(),
    );
    let input_numerator = if input_is_one {
        1_u128 << 32
    } else {
        input_limbs
            .iter()
            .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
    };
    let threshold_numerator = half_threshold_numerator(2);
    z.push(
        AllocatedNum::alloc(cs.namespace(|| "input_decision"), || {
            Ok(Scalar::from(u64::from(
                input_numerator >= threshold_numerator,
            )))
        })
        .unwrap(),
    );
    let circuit = LogisticStepCircuit {
        witness,
        r_num: 37,
        r_den: 10,
        rounding,
        threshold_numerator,
    };
    circuit.synthesize(&mut cs, &z).unwrap();
    cs
}

fn synthesize_cdad_score(
    witness: CdadLinearScoreWitness,
    selected_precision: u64,
) -> TestConstraintSystem<Scalar> {
    let mut cs = TestConstraintSystem::new();
    let z = vec![
        AllocatedNum::alloc(cs.namespace(|| "score_iteration"), || Ok(Scalar::ZERO)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "score_previous_decision"), || {
            Ok(Scalar::from(2_u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "score_selected_precision"), || {
            Ok(Scalar::from(selected_precision))
        })
        .unwrap(),
    ];
    CdadLinearScoreCircuit { witness }
        .synthesize(&mut cs, &z)
        .unwrap();
    cs
}

fn synthesize_compensated(
    input_limbs: [u64; 2],
    input_is_one: bool,
    input_residual_offset: u64,
    witness: CompensatedStepWitness,
    rounding: CircuitRounding,
) -> TestConstraintSystem<Scalar> {
    let mut cs = TestConstraintSystem::new();
    let z = vec![
        AllocatedNum::alloc(cs.namespace(|| "comp_iteration"), || Ok(Scalar::ZERO)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "comp_input_0"), || {
            Ok(Scalar::from(input_limbs[0]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "comp_input_1"), || {
            Ok(Scalar::from(input_limbs[1]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "comp_input_is_one"), || {
            Ok(Scalar::from(input_is_one as u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "comp_input_residual_offset"), || {
            Ok(Scalar::from(input_residual_offset))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "comp_input_decision"), || {
            Ok(Scalar::from(u64::from(
                comp_fine_numerator(input_limbs, input_is_one, input_residual_offset)
                    >= COMPENSATED_HALF,
            )))
        })
        .unwrap(),
    ];
    let circuit = CompensatedLogisticStepCircuit {
        witness,
        r_num: 37,
        r_den: 10,
        rounding,
        threshold_fine_numerator: COMPENSATED_HALF,
    };
    circuit.synthesize(&mut cs, &z).unwrap();
    cs
}

fn synthesize_carry(
    input_limbs: [u64; 2],
    input_is_one: bool,
    witness: StepWitness<2>,
    carry_witness: ProductCarryWitness2,
    rounding: CircuitRounding,
) -> TestConstraintSystem<Scalar> {
    let mut cs = TestConstraintSystem::new();
    let z = vec![
        AllocatedNum::alloc(cs.namespace(|| "carry_iteration"), || Ok(Scalar::ZERO)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "carry_input_0"), || {
            Ok(Scalar::from(input_limbs[0]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "carry_input_1"), || {
            Ok(Scalar::from(input_limbs[1]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "carry_input_is_one"), || {
            Ok(Scalar::from(input_is_one as u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "carry_input_decision"), || {
            let numerator = if input_is_one {
                1_u128 << 32
            } else {
                input_limbs
                    .iter()
                    .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
            };
            Ok(Scalar::from(u64::from(
                numerator >= half_threshold_numerator(2),
            )))
        })
        .unwrap(),
    ];
    let circuit = CarryLogisticStepCircuit2 {
        witness,
        carry_witness,
        r_num: 37,
        r_den: 10,
        rounding,
        threshold_numerator: half_threshold_numerator(2),
    };
    circuit.synthesize(&mut cs, &z).unwrap();
    cs
}

fn synthesize_cbd(
    input_limbs: [u64; 2],
    input_is_one: bool,
    input_radius: u128,
    witness: CbdStepWitness<2>,
) -> TestConstraintSystem<Scalar> {
    let mut cs = TestConstraintSystem::new();
    let z = vec![
        AllocatedNum::alloc(cs.namespace(|| "cbd_iteration"), || Ok(Scalar::ZERO)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_input_0"), || {
            Ok(Scalar::from(input_limbs[0]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_input_1"), || {
            Ok(Scalar::from(input_limbs[1]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_input_is_one"), || {
            Ok(Scalar::from(input_is_one as u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_input_radius"), || {
            Ok(Scalar::from(input_radius as u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_input_decision"), || Ok(Scalar::ZERO)).unwrap(),
    ];
    let circuit = CbdLogisticStepCircuit {
        witness,
        r_num: 37,
        r_den: 10,
        radius_bound: CbdRadiusBound::Global,
        threshold_numerator: half_threshold_numerator(2),
    };
    circuit.synthesize(&mut cs, &z).unwrap();
    cs
}

fn synthesize_cdad(selected_bits: u64) -> TestConstraintSystem<Scalar> {
    let input = [0x1999, 0x999a];
    let radius = 1_u128;
    let witness = CbdStepWitness::compute_with_threshold(
        input,
        false,
        radius,
        37,
        10,
        CbdRadiusBound::LocalTaylor,
        half_threshold_numerator(2),
    );
    let inner = CbdLogisticStepCircuit {
        witness,
        r_num: 37,
        r_den: 10,
        radius_bound: CbdRadiusBound::LocalTaylor,
        threshold_numerator: half_threshold_numerator(2),
    };
    let circuit = CdadFixedLogisticStepCircuit::new(inner);
    let mut cs = TestConstraintSystem::new();
    let values = [
        0_u64,
        input[0],
        input[1],
        0,
        radius as u64,
        0,
        selected_bits,
    ];
    let z = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cs.namespace(|| format!("cdad_state_{index}")), || {
                Ok(Scalar::from(*value))
            })
            .unwrap()
        })
        .collect::<Vec<_>>();
    circuit.synthesize(&mut cs, &z).unwrap();
    cs
}

fn synthesize_cbd_sequential(
    input_limbs: [u64; 2],
    input_is_one: bool,
    input_radius: u128,
    witness: CbdSequentialStepWitness<2>,
) -> TestConstraintSystem<Scalar> {
    let mut cs = TestConstraintSystem::new();
    let z = vec![
        AllocatedNum::alloc(cs.namespace(|| "cbd_seq_iteration"), || Ok(Scalar::ZERO)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_seq_input_0"), || {
            Ok(Scalar::from(input_limbs[0]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_seq_input_1"), || {
            Ok(Scalar::from(input_limbs[1]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_seq_input_is_one"), || {
            Ok(Scalar::from(input_is_one as u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_seq_input_radius"), || {
            Ok(Scalar::from(input_radius as u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cbd_seq_input_decision"), || {
            Ok(Scalar::ZERO)
        })
        .unwrap(),
    ];
    CbdSequentialLogisticStepCircuit {
        witness,
        r_num: 37,
        r_den: 10,
        threshold_numerator: half_threshold_numerator(2),
    }
    .synthesize(&mut cs, &z)
    .unwrap();
    cs
}

fn synthesize_cfd(
    profile: CfdProfile,
    input: &CertifiedFloat,
    witness: CfdLogisticStepWitness,
    topology: CfdTopology,
) -> TestConstraintSystem<Scalar> {
    let mut cs = TestConstraintSystem::new();
    let threshold_common =
        common_threshold(&profile, &BigRational::new(1.into(), 2.into())).unwrap();
    let input_decision = circuit_decision(&profile, input, threshold_common).unwrap();
    let z = vec![
        AllocatedNum::alloc(cs.namespace(|| "cfd_iteration"), || Ok(Scalar::ZERO)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cfd_lane"), || {
            Ok(Scalar::from(input.exponent_lane() as u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cfd_significand"), || {
            Ok(Scalar::from(input.significand().to_u64().unwrap()))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cfd_radius"), || {
            Ok(Scalar::from(input.radius().to_u64().unwrap()))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cfd_zero"), || {
            Ok(Scalar::from(input.is_zero() as u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cfd_wide"), || {
            Ok(Scalar::from(input.is_wide() as u64))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "cfd_decision"), || {
            Ok(Scalar::from(match input_decision {
                Decision3::False => 0,
                Decision3::True => 1,
                Decision3::Indeterminate => 2,
            }))
        })
        .unwrap(),
    ];
    CfdLogisticStepCircuit {
        profile,
        witness,
        r_num: 37,
        r_den: 10,
        topology,
        threshold_common,
    }
    .synthesize(&mut cs, &z)
    .unwrap();
    cs
}

fn synthesize_cfd_stable(
    profile: CfdProfile,
    input: &CertifiedFloat,
    witness: CfdStableStepWitness,
) -> TestConstraintSystem<Scalar> {
    let mut cs = TestConstraintSystem::new();
    let threshold_common =
        common_threshold(&profile, &BigRational::from_integer(1.into())).unwrap();
    let decision = circuit_decision(&profile, input, threshold_common).unwrap();
    let values = [
        0_u64,
        input.exponent_lane() as u64,
        input.significand().to_u64().unwrap(),
        input.radius().to_u64().unwrap(),
        input.is_zero() as u64,
        input.is_wide() as u64,
        match decision {
            Decision3::False => 0,
            Decision3::True => 1,
            Decision3::Indeterminate => 2,
        },
    ];
    let z = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cs.namespace(|| format!("stable_z_{index}")), || {
                Ok(Scalar::from(value))
            })
            .unwrap()
        })
        .collect::<Vec<_>>();
    CfdStableStepCircuit {
        profile,
        witness,
        alpha_num: 1,
        alpha_den: 2,
        innovation_num: 3,
        innovation_den: 4,
        threshold_common,
    }
    .synthesize(&mut cs, &z)
    .unwrap();
    cs
}

#[test]
fn accepts_valid_transition() {
    let input = [0x1999, 0x999a]; // nearest encoding of 0.1
    let witness = StepWitness::compute(input, false, 37, 10);
    let cs = synthesize(input, false, witness, CircuitRounding::Truncate);
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn accepts_nearest_even_transition() {
    let input = [0x1999, 0x999a]; // nearest encoding of 0.1
    let witness = StepWitness::compute_with_rounding(
        input,
        false,
        37,
        10,
        CircuitRounding::NearestTiesToEven,
    );
    let cs = synthesize(input, false, witness, CircuitRounding::NearestTiesToEven);
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn rejects_tampered_remainder() {
    let input = [0x1999, 0x999a];
    let mut witness = StepWitness::compute(input, false, 37, 10);
    witness.remainder_product += 1;
    let cs = synthesize(input, false, witness, CircuitRounding::Truncate);
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_tampered_nearest_even_rounding_bit() {
    let input = [0x1999, 0x999a];
    let mut witness = StepWitness::compute_with_rounding(
        input,
        false,
        37,
        10,
        CircuitRounding::NearestTiesToEven,
    );
    witness.product_rounded_up = !witness.product_rounded_up;
    witness.quotient_product = witness.product_floor + u128::from(witness.product_rounded_up);
    let cs = synthesize(input, false, witness, CircuitRounding::NearestTiesToEven);
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_tampered_output() {
    let input = [0x1999, 0x999a];
    let mut witness = StepWitness::compute(input, false, 37, 10);
    witness.output_limbs[1] ^= 1;
    let cs = synthesize(input, false, witness, CircuitRounding::Truncate);
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_limb_out_of_range() {
    let input = [0x1_0000, 0];
    let witness = StepWitness::compute(input, false, 37, 10);
    let cs = synthesize(input, false, witness, CircuitRounding::Truncate);
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_tampered_input_decomposition() {
    let input = [0x1999, 0x9999];
    let mut witness = StepWitness::compute(input, false, 37, 10);
    witness.input_limbs[1] ^= 1;
    let cs = synthesize(input, false, witness, CircuitRounding::Truncate);
    assert!(!cs.is_satisfied());
}

#[test]
fn accepts_compensated_transition() {
    let input = [0x1999, 0x999a];
    let residual_offset = 1_u64 << 31; // residual zero
    let witness = CompensatedStepWitness::compute(input, false, residual_offset, 37, 10);
    let cs = synthesize_compensated(
        input,
        false,
        residual_offset,
        witness,
        CircuitRounding::Truncate,
    );
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn rejects_tampered_compensated_residual() {
    let input = [0x1999, 0x999a];
    let residual_offset = 1_u64 << 31;
    let mut witness = CompensatedStepWitness::compute(input, false, residual_offset, 37, 10);
    witness.output_residual_offset ^= 1;
    let cs = synthesize_compensated(
        input,
        false,
        residual_offset,
        witness,
        CircuitRounding::Truncate,
    );
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_tampered_compensated_remainder() {
    let input = [0x1999, 0x999a];
    let residual_offset = 1_u64 << 31;
    let mut witness = CompensatedStepWitness::compute(input, false, residual_offset, 37, 10);
    witness.fine_remainder += 1;
    let cs = synthesize_compensated(
        input,
        false,
        residual_offset,
        witness,
        CircuitRounding::Truncate,
    );
    assert!(!cs.is_satisfied());
}

#[test]
fn accepts_compensated_nearest_even_transition() {
    let input = [0x1999, 0x999a];
    let residual_offset = 1_u64 << 31;
    let witness = CompensatedStepWitness::compute_with_rounding(
        input,
        false,
        residual_offset,
        37,
        10,
        CircuitRounding::NearestTiesToEven,
    );
    let cs = synthesize_compensated(
        input,
        false,
        residual_offset,
        witness,
        CircuitRounding::NearestTiesToEven,
    );
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn rejects_tampered_compensated_rounding_bit() {
    let input = [0x1999, 0x999a];
    let residual_offset = 1_u64 << 31;
    let mut witness = CompensatedStepWitness::compute_with_rounding(
        input,
        false,
        residual_offset,
        37,
        10,
        CircuitRounding::NearestTiesToEven,
    );
    witness.fine_rounded_up = !witness.fine_rounded_up;
    let cs = synthesize_compensated(
        input,
        false,
        residual_offset,
        witness,
        CircuitRounding::NearestTiesToEven,
    );
    assert!(!cs.is_satisfied());
}

#[test]
fn accepts_carry_transition() {
    let input = [0x1999, 0x999a];
    let witness = StepWitness::compute(input, false, 37, 10);
    let carry_witness = ProductCarryWitness2::compute(input, false);
    let cs = synthesize_carry(
        input,
        false,
        witness,
        carry_witness,
        CircuitRounding::Truncate,
    );
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn rejects_tampered_product_carry() {
    let input = [0x1999, 0x999a];
    let witness = StepWitness::compute(input, false, 37, 10);
    let mut carry_witness = ProductCarryWitness2::compute(input, false);
    carry_witness.carries[1] += 1;
    let cs = synthesize_carry(
        input,
        false,
        witness,
        carry_witness,
        CircuitRounding::Truncate,
    );
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_tampered_product_digit() {
    let input = [0x1999, 0x999a];
    let witness = StepWitness::compute(input, false, 37, 10);
    let mut carry_witness = ProductCarryWitness2::compute(input, false);
    carry_witness.product_digits_le[0] ^= 1;
    let cs = synthesize_carry(
        input,
        false,
        witness,
        carry_witness,
        CircuitRounding::Truncate,
    );
    assert!(!cs.is_satisfied());
}

#[test]
fn accepts_carry_zero_endpoint() {
    let input = [0, 0];
    let witness = StepWitness::compute(input, false, 37, 10);
    let carry_witness = ProductCarryWitness2::compute(input, false);
    assert!(carry_witness.complement_is_one);
    let cs = synthesize_carry(
        input,
        false,
        witness,
        carry_witness,
        CircuitRounding::Truncate,
    );
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn accepts_carry_one_endpoint() {
    let input = [0, 0];
    let witness = StepWitness::compute(input, true, 37, 10);
    let carry_witness = ProductCarryWitness2::compute(input, true);
    assert!(!carry_witness.complement_is_one);
    let cs = synthesize_carry(
        input,
        true,
        witness,
        carry_witness,
        CircuitRounding::Truncate,
    );
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn rejects_tampered_complement_is_one() {
    let input = [0, 0];
    let witness = StepWitness::compute(input, false, 37, 10);
    let mut carry_witness = ProductCarryWitness2::compute(input, false);
    carry_witness.complement_is_one = false;
    let cs = synthesize_carry(
        input,
        false,
        witness,
        carry_witness,
        CircuitRounding::Truncate,
    );
    assert!(!cs.is_satisfied());
}

#[test]
fn accepts_cbd_fused_transition() {
    let input = [0x1999, 0x999a];
    let witness = CbdStepWitness::compute(input, false, 1, 37, 10);
    let cs = synthesize_cbd(input, false, 1, witness);
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn accepts_cdad_fixed_precision_transition() {
    let cs = synthesize_cdad(32);
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn rejects_cdad_precision_outside_compiled_rung() {
    let cs = synthesize_cdad(48);
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_tampered_cbd_center_remainder() {
    let input = [0x1999, 0x999a];
    let mut witness = CbdStepWitness::compute(input, false, 1, 37, 10);
    witness.center_remainder += 1;
    let cs = synthesize_cbd(input, false, 1, witness);
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_tampered_cbd_output_radius() {
    let input = [0x1999, 0x999a];
    let mut witness = CbdStepWitness::compute(input, false, 1, 37, 10);
    witness.output_radius += 1;
    let cs = synthesize_cbd(input, false, 1, witness);
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_tampered_cbd_radius_remainder() {
    let input = [0x1999, 0x999a];
    let mut witness = CbdStepWitness::compute(input, false, 1, 37, 10);
    witness.radius_remainder = (witness.radius_remainder + 1) % 10;
    let cs = synthesize_cbd(input, false, 1, witness);
    assert!(!cs.is_satisfied());
}

#[test]
fn rejects_tampered_cbd_decision() {
    let input = [0x1999, 0x999a];
    let mut witness = CbdStepWitness::compute_local(input, false, 1, 37, 10);
    witness.decision_half = 1;
    let mut cs = TestConstraintSystem::new();
    let z = vec![
        AllocatedNum::alloc(cs.namespace(|| "decision_iteration"), || Ok(Scalar::ZERO)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "decision_input_0"), || {
            Ok(Scalar::from(input[0]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "decision_input_1"), || {
            Ok(Scalar::from(input[1]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "decision_input_is_one"), || {
            Ok(Scalar::ZERO)
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "decision_input_radius"), || Ok(Scalar::ONE)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "decision_input_code"), || Ok(Scalar::ZERO)).unwrap(),
    ];
    let circuit = CbdLogisticStepCircuit {
        witness,
        r_num: 37,
        r_den: 10,
        radius_bound: CbdRadiusBound::LocalTaylor,
        threshold_numerator: half_threshold_numerator(2),
    };
    circuit.synthesize(&mut cs, &z).unwrap();
    assert!(!cs.is_satisfied());
}

#[test]
fn cbd_radius_saturates_to_full_domain() {
    let input = [0x8000, 0x0000];
    let scale = 1_u128 << 32;
    let witness = CbdStepWitness::compute(input, false, scale, 37, 10);
    assert!(witness.radius_capped);
    assert_eq!(witness.output_radius, scale);
    let cs = synthesize_cbd(input, false, scale, witness);
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn cbd_circuit_witness_matches_off_circuit_semantics() {
    let exact = BigRational::new(1.into(), 10.into());
    let r = BigRational::new(37.into(), 10.into());
    let input =
        CertifiedDyadic::from_exact(NumericProfile::bounded_unit(32).unwrap(), &exact).unwrap();
    let input_num = input.mantissa().to_u128().unwrap();
    let limbs = [(input_num >> 16) as u64, (input_num & 0xffff) as u64];
    let witness = CbdStepWitness::compute(limbs, false, input.radius().to_u128().unwrap(), 37, 10);
    let output = input.logistic_step_lipschitz(&r).unwrap();
    let witness_output = if witness.output_is_one {
        1_u128 << 32
    } else {
        ((witness.output_limbs[0] as u128) << 16) + witness.output_limbs[1] as u128
    };
    assert_eq!(output.mantissa().to_u128().unwrap(), witness_output);
    assert_eq!(output.radius().to_u128().unwrap(), witness.output_radius);
}

#[test]
fn accepts_cbd_local_bound_and_matches_off_circuit() {
    let exact = BigRational::new(1.into(), 10.into());
    let r = BigRational::new(37.into(), 10.into());
    let input =
        CertifiedDyadic::from_exact(NumericProfile::bounded_unit(32).unwrap(), &exact).unwrap();
    let input_num = input.mantissa().to_u128().unwrap();
    let limbs = [(input_num >> 16) as u64, (input_num & 0xffff) as u64];
    let witness =
        CbdStepWitness::compute_local(limbs, false, input.radius().to_u128().unwrap(), 37, 10);
    let output = input.logistic_step_local_bound(&r).unwrap();
    let witness_output =
        ((witness.output_limbs[0] as u128) << 16) + witness.output_limbs[1] as u128;
    assert_eq!(output.mantissa().to_u128().unwrap(), witness_output);
    assert_eq!(output.radius().to_u128().unwrap(), witness.output_radius);

    let mut cs = TestConstraintSystem::new();
    let z = vec![
        AllocatedNum::alloc(cs.namespace(|| "local_iteration"), || Ok(Scalar::ZERO)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "local_input_0"), || {
            Ok(Scalar::from(limbs[0]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "local_input_1"), || {
            Ok(Scalar::from(limbs[1]))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "local_input_is_one"), || Ok(Scalar::ZERO)).unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "local_input_radius"), || {
            Ok(Scalar::from(input.radius().to_u64().unwrap()))
        })
        .unwrap(),
        AllocatedNum::alloc(cs.namespace(|| "local_input_decision"), || Ok(Scalar::ZERO)).unwrap(),
    ];
    let circuit = CbdLogisticStepCircuit {
        witness,
        r_num: 37,
        r_den: 10,
        radius_bound: CbdRadiusBound::LocalTaylor,
        threshold_numerator: half_threshold_numerator(2),
    };
    circuit.synthesize(&mut cs, &z).unwrap();
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn accepts_cbd_sequential_and_matches_off_circuit() {
    let exact = BigRational::new(1.into(), 10.into());
    let r = BigRational::new(37.into(), 10.into());
    let input =
        CertifiedDyadic::from_exact(NumericProfile::bounded_unit(32).unwrap(), &exact).unwrap();
    let input_num = input.mantissa().to_u128().unwrap();
    let limbs = [(input_num >> 16) as u64, (input_num & 0xffff) as u64];
    let witness =
        CbdSequentialStepWitness::compute(limbs, false, input.radius().to_u128().unwrap(), 37, 10);
    let output = input.logistic_step_sequential_local_bound(&r).unwrap();
    let witness_output =
        ((witness.output_limbs[0] as u128) << 16) + witness.output_limbs[1] as u128;
    assert_eq!(output.mantissa().to_u128().unwrap(), witness_output);
    assert_eq!(output.radius().to_u128().unwrap(), witness.output_radius);
    let cs = synthesize_cbd_sequential(limbs, false, input.radius().to_u128().unwrap(), witness);
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
}

#[test]
fn rejects_tampered_cbd_sequential_intermediates() {
    let input = [0x1999, 0x999a];

    let mut product_remainder = CbdSequentialStepWitness::compute(input, false, 1, 37, 10);
    product_remainder.product_remainder ^= 1;
    assert!(!synthesize_cbd_sequential(input, false, 1, product_remainder).is_satisfied());

    let mut product_rounding = CbdSequentialStepWitness::compute(input, false, 1, 37, 10);
    product_rounding.product_rounded_up = !product_rounding.product_rounded_up;
    assert!(!synthesize_cbd_sequential(input, false, 1, product_rounding).is_satisfied());

    let mut product_radius = CbdSequentialStepWitness::compute(input, false, 1, 37, 10);
    product_radius.product_radius += 1;
    assert!(!synthesize_cbd_sequential(input, false, 1, product_radius).is_satisfied());

    let mut output_remainder = CbdSequentialStepWitness::compute(input, false, 1, 37, 10);
    output_remainder.output_remainder = (output_remainder.output_remainder + 1) % 10;
    assert!(!synthesize_cbd_sequential(input, false, 1, output_remainder).is_satisfied());

    let mut output_radius = CbdSequentialStepWitness::compute(input, false, 1, 37, 10);
    output_radius.output_radius += 1;
    assert!(!synthesize_cbd_sequential(input, false, 1, output_radius).is_satisfied());

    let mut decision = CbdSequentialStepWitness::compute(input, false, 1, 37, 10);
    decision.decision_half = (decision.decision_half + 1) % 3;
    assert!(!synthesize_cbd_sequential(input, false, 1, decision).is_satisfied());
}

#[test]
fn cbd_sequential_random_witnesses_match_reference() {
    let profile = NumericProfile::bounded_unit(32).unwrap();
    let r = BigRational::new(37.into(), 10.into());
    let mut state = 0x4342_4453_u32;
    for index in 0..256 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let exact = BigRational::new((state % 1_000_001).into(), 1_000_000_u32.into());
        let input = CertifiedDyadic::from_exact(profile.clone(), &exact).unwrap();
        let input_num = input.mantissa().to_u128().unwrap();
        let input_is_one = input_num == (1_u128 << 32);
        let limbs = if input_is_one {
            [0, 0]
        } else {
            [(input_num >> 16) as u64, (input_num & 0xffff) as u64]
        };
        let witness = CbdSequentialStepWitness::compute(
            limbs,
            input_is_one,
            input.radius().to_u128().unwrap(),
            37,
            10,
        );
        let output = input.logistic_step_sequential_local_bound(&r).unwrap();
        let witness_output =
            ((witness.output_limbs[0] as u128) << 16) + witness.output_limbs[1] as u128;
        assert_eq!(output.mantissa().to_u128().unwrap(), witness_output);
        assert_eq!(output.radius().to_u128().unwrap(), witness.output_radius);
        if index < 64 {
            let cs = synthesize_cbd_sequential(
                limbs,
                input_is_one,
                input.radius().to_u128().unwrap(),
                witness,
            );
            assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
        }
    }
}

#[test]
fn cbd_sequential_shape_is_independent_of_witness() {
    let cases = [
        ([0_u64, 0_u64], false, 0_u128),
        ([0x1999, 0x999a], false, 1_u128),
        ([0x8000, 0], false, 1_u128),
        ([0, 0], true, 0_u128),
    ];
    let mut expected = None;
    for (limbs, is_one, radius) in cases {
        let witness = CbdSequentialStepWitness::compute(limbs, is_one, radius, 37, 10);
        let cs = synthesize_cbd_sequential(limbs, is_one, radius, witness);
        assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
        if let Some(count) = expected {
            assert_eq!(cs.num_constraints(), count);
        } else {
            expected = Some(cs.num_constraints());
        }
    }
}

#[test]
fn accepts_cfd_sequential_and_fused_enclosures() {
    let profile = CfdProfile::bounded_unit(24).unwrap();
    let input = CertifiedFloat::from_exact(profile.clone(), &BigRational::new(1.into(), 10.into()))
        .unwrap();
    for topology in [CfdTopology::Sequential, CfdTopology::Fused] {
        let witness = CfdLogisticStepWitness::compute(&profile, &input, 37, 10, topology).unwrap();
        let cs = synthesize_cfd(profile.clone(), &input, witness, topology);
        assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    }
}

#[test]
fn rejects_cfd_tampered_center_radius_and_decision() {
    let profile = CfdProfile::bounded_unit(24).unwrap();
    let input = CertifiedFloat::from_exact(profile.clone(), &BigRational::new(1.into(), 10.into()))
        .unwrap();

    let mut center =
        CfdLogisticStepWitness::compute(&profile, &input, 37, 10, CfdTopology::Sequential).unwrap();
    center.output.significand += 10_000;
    assert!(
        !synthesize_cfd(profile.clone(), &input, center, CfdTopology::Sequential).is_satisfied()
    );

    let mut radius =
        CfdLogisticStepWitness::compute(&profile, &input, 37, 10, CfdTopology::Sequential).unwrap();
    radius.output.radius = 0;
    assert!(
        !synthesize_cfd(profile.clone(), &input, radius, CfdTopology::Sequential).is_satisfied()
    );

    let mut decision =
        CfdLogisticStepWitness::compute(&profile, &input, 37, 10, CfdTopology::Fused).unwrap();
    decision.decision = Decision3::True;
    assert!(!synthesize_cfd(profile, &input, decision, CfdTopology::Fused).is_satisfied());
}

#[test]
fn cfd_shape_is_independent_of_numeric_witness() {
    let profile = CfdProfile::bounded_unit(24).unwrap();
    let mut expected = None;
    for exact in [
        BigRational::new(1.into(), 10.into()),
        BigRational::new(1.into(), 4.into()),
        BigRational::new(3.into(), 4.into()),
    ] {
        let input = CertifiedFloat::from_exact(profile.clone(), &exact).unwrap();
        let witness =
            CfdLogisticStepWitness::compute(&profile, &input, 37, 10, CfdTopology::Sequential)
                .unwrap();
        let cs = synthesize_cfd(profile.clone(), &input, witness, CfdTopology::Sequential);
        assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
        if let Some(count) = expected {
            assert_eq!(cs.num_constraints(), count);
        } else {
            expected = Some(cs.num_constraints());
        }
    }
}

#[test]
fn accepts_stable_cfd_transition_and_rejects_tampering() {
    let profile = CfdProfile::stable_score(24).unwrap();
    let input = CertifiedFloat::from_exact(profile.clone(), &BigRational::new(1.into(), 10.into()))
        .unwrap();
    let witness = CfdStableStepWitness::compute(&profile, &input, 1, 2, 3, 4).unwrap();
    let cs = synthesize_cfd_stable(profile.clone(), &input, witness);
    assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());

    let mut tampered = CfdStableStepWitness::compute(&profile, &input, 1, 2, 3, 4).unwrap();
    tampered.output.significand += 50_000;
    let cs = synthesize_cfd_stable(profile, &input, tampered);
    assert!(!cs.is_satisfied());
}

fn score_witness() -> CdadLinearScoreWitness {
    score_witness_at(16)
}

fn score_witness_at(bits: usize) -> CdadLinearScoreWitness {
    let scale = 1_u128 << bits;
    CdadLinearScoreWitness::compute(
        bits,
        SignedCdadValue {
            negative: false,
            magnitude: scale / 10,
            radius: 1,
        },
        vec![(
            SignedCdadValue {
                negative: false,
                magnitude: scale / 2,
                radius: 1,
            },
            SignedCdadValue {
                negative: false,
                magnitude: scale / 2,
                radius: 1,
            },
        )],
        BigRational::new(1.into(), 5.into()),
    )
}

#[test]
fn accepts_cdad_signed_score_and_selected_precision() {
    for bits in [16_u64, 32, 48, 64] {
        let witness = score_witness_at(bits as usize);
        assert_eq!(witness.decision, 1);
        let cs = synthesize_cdad_score(witness, bits);
        assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    }
}

#[test]
fn rejects_tampered_cdad_score_fields_and_precision() {
    let original = score_witness();

    let mut center = original.clone();
    center.output_center_offset += 1;
    assert!(!synthesize_cdad_score(center, 16).is_satisfied());

    let mut radius = original.clone();
    radius.output_radius += 1;
    assert!(!synthesize_cdad_score(radius, 16).is_satisfied());

    let mut decision = original.clone();
    decision.decision = 0;
    assert!(!synthesize_cdad_score(decision, 16).is_satisfied());

    assert!(!synthesize_cdad_score(original, 32).is_satisfied());
}

#[test]
fn rejects_noncanonical_negative_zero_in_cdad_score() {
    let mut witness = score_witness();
    witness.terms[0].0.magnitude = 0;
    witness.terms[0].0.negative = true;
    witness = CdadLinearScoreWitness::compute(
        witness.fractional_bits,
        witness.bias,
        witness.terms,
        witness.threshold,
    );
    let cs = synthesize_cdad_score(witness, 16);
    assert!(!cs.is_satisfied());
}

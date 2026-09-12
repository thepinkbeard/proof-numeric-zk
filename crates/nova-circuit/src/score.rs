//! Signed, range-checked inner product for Adult/ECEF CDAD decisions.

use super::{
    allocate_boolean_num, allocate_nonzero_indicator_from_bits, allocate_ranged,
    enforce_certified_decision, enforce_constant_divisor_rounding, field_from_u128,
    CertifiedDecisionArgs, CircuitRounding, ConstantDivisorRoundingArgs,
};
use ff::PrimeField;
use nova_snark::{
    frontend::{num::AllocatedNum, ConstraintSystem, SynthesisError},
    traits::circuit::StepCircuit,
};
use num_bigint::{BigInt, BigUint};
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedCdadValue {
    pub negative: bool,
    pub magnitude: u128,
    pub radius: u128,
}

impl SignedCdadValue {
    pub fn from_parts(mantissa: &BigInt, radius: &BigUint) -> Self {
        Self {
            negative: mantissa.is_negative(),
            magnitude: mantissa
                .magnitude()
                .to_u128()
                .expect("mantissa CDAD cabe em u128"),
            radius: radius.to_u128().expect("raio CDAD cabe em u128"),
        }
    }

    fn signed(&self) -> i128 {
        if self.negative {
            -(self.magnitude as i128)
        } else {
            self.magnitude as i128
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CdadLinearScoreWitness {
    pub fractional_bits: usize,
    pub bias: SignedCdadValue,
    pub terms: Vec<(SignedCdadValue, SignedCdadValue)>,
    pub threshold: BigRational,
    pub output_center_offset: u128,
    pub output_radius: u128,
    pub decision: u64,
}

impl CdadLinearScoreWitness {
    pub fn compute(
        fractional_bits: usize,
        bias: SignedCdadValue,
        terms: Vec<(SignedCdadValue, SignedCdadValue)>,
        threshold: BigRational,
    ) -> Self {
        assert!((1..=64).contains(&fractional_bits));
        let scale = 1_u128 << fractional_bits;
        let scale_big = BigUint::from(scale);
        let offset = 64_u128 * scale;
        let mut center = bias.signed();
        let mut radius = bias.radius;
        for (left, right) in &terms {
            let raw = BigUint::from(left.magnitude) * BigUint::from(right.magnitude);
            let floor = (&raw / &scale_big)
                .to_u128()
                .expect("produto arredondado cabe em u128");
            let remainder = (&raw % &scale_big)
                .to_u128()
                .expect("resto do produto cabe em u128");
            let rounded_up = remainder > scale / 2 || (remainder == scale / 2 && floor & 1 == 1);
            let magnitude = floor + u128::from(rounded_up);
            let negative = left.negative ^ right.negative;
            center += if negative {
                -(magnitude as i128)
            } else {
                magnitude as i128
            };
            let error_numerator = BigUint::from(left.magnitude) * BigUint::from(right.radius)
                + BigUint::from(right.magnitude) * BigUint::from(left.radius)
                + BigUint::from(left.radius) * BigUint::from(right.radius);
            let error_floor = (&error_numerator / &scale_big)
                .to_u128()
                .expect("erro propagado cabe em u128");
            let error_remainder = (&error_numerator % &scale_big)
                .to_u128()
                .expect("resto do erro cabe em u128");
            let propagated =
                error_floor + u128::from(error_remainder != 0) + u128::from(remainder != 0);
            radius += propagated;
        }
        assert!(center >= -(offset as i128) && center <= offset as i128);
        let output_center_offset = (center + offset as i128) as u128;
        assert!(output_center_offset >= radius && output_center_offset + radius <= 2 * offset);
        let threshold_scaled = &threshold * BigRational::from_integer(BigInt::from(scale));
        let threshold_ceiling = {
            let quotient = threshold_scaled.numer() / threshold_scaled.denom();
            let remainder = threshold_scaled.numer() % threshold_scaled.denom();
            let ceiling = if remainder == BigInt::from(0) {
                quotient
            } else if threshold_scaled.is_positive() {
                quotient + 1
            } else {
                quotient
            };
            ceiling.to_i128().expect("limiar escalado cabe em i128")
        };
        let threshold_offset = (offset as i128 + threshold_ceiling) as u128;
        let decision = if output_center_offset >= threshold_offset + radius {
            1
        } else if output_center_offset + radius < threshold_offset {
            0
        } else {
            2
        };
        Self {
            fractional_bits,
            bias,
            terms,
            threshold,
            output_center_offset,
            output_radius: radius,
            decision,
        }
    }

    fn threshold_offset(&self) -> u128 {
        let scale = 1_u128 << self.fractional_bits;
        let offset = 64_u128 * scale;
        let scaled = &self.threshold * BigRational::from_integer(BigInt::from(scale));
        let quotient = scaled.numer() / scaled.denom();
        let remainder = scaled.numer() % scaled.denom();
        let ceiling = if remainder == BigInt::from(0) {
            quotient
        } else if scaled.is_positive() {
            quotient + 1
        } else {
            quotient
        };
        (offset as i128 + ceiling.to_i128().expect("limiar cabe")) as u128
    }
}

#[derive(Clone, Debug)]
pub struct CdadLinearScoreCircuit {
    pub witness: CdadLinearScoreWitness,
}

struct AllocatedSigned<F: PrimeField> {
    signed: AllocatedNum<F>,
    magnitude: AllocatedNum<F>,
    radius: AllocatedNum<F>,
    negative: AllocatedNum<F>,
}

fn field_from_i128<F: PrimeField>(value: i128) -> F {
    if value < 0 {
        -field_from_u128::<F>(value.unsigned_abs())
    } else {
        field_from_u128(value as u128)
    }
}

fn allocate_signed<F, CS>(
    mut cs: CS,
    name: &str,
    value: &SignedCdadValue,
    magnitude_bits: usize,
) -> Result<AllocatedSigned<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let (magnitude, magnitude_decomposition) = allocate_ranged(
        cs.namespace(|| format!("{name}_magnitude")),
        "signed_magnitude",
        value.magnitude,
        magnitude_bits,
    )?;
    let (radius, _) = allocate_ranged(
        cs.namespace(|| format!("{name}_radius")),
        "signed_radius",
        value.radius,
        magnitude_bits,
    )?;
    let negative = allocate_boolean_num(
        cs.namespace(|| format!("{name}_negative")),
        "signed_negative",
        value.negative,
    )?;
    let magnitude_nonzero = allocate_nonzero_indicator_from_bits(
        cs.namespace(|| format!("{name}_magnitude_nonzero")),
        &magnitude_decomposition,
    )?;
    cs.enforce(
        || format!("{name} rejects negative zero"),
        |lc| lc + negative.get_variable(),
        |lc| lc + CS::one() - magnitude_nonzero.get_variable(),
        |lc| lc,
    );
    let signed = AllocatedNum::alloc(cs.namespace(|| format!("{name}_signed")), || {
        Ok(field_from_i128(value.signed()))
    })?;
    cs.enforce(
        || format!("{name} sign-magnitude relation"),
        |lc| lc + magnitude.get_variable(),
        |lc| lc + CS::one() - (F::from(2), negative.get_variable()),
        |lc| lc + signed.get_variable(),
    );
    Ok(AllocatedSigned {
        signed,
        magnitude,
        radius,
        negative,
    })
}

impl CdadLinearScoreCircuit {
    pub fn arity_value() -> usize {
        3 // iteration, decision, selected precision
    }
}

impl<F: PrimeField> StepCircuit<F> for CdadLinearScoreCircuit {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert_eq!(z.len(), Self::arity_value());
        let bits = self.witness.fractional_bits;
        assert!((1..=64).contains(&bits));
        cs.enforce(
            || "score precision belongs to compiled profile",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[2].get_variable() - (F::from(bits as u64), CS::one()),
        );
        let scale_u128 = 1_u128 << bits;
        let scale_big = BigUint::from(scale_u128);
        let scale = field_from_u128::<F>(scale_u128);
        let value_bits = bits + 8;
        let bias = allocate_signed(
            cs.namespace(|| "score_bias"),
            "score_bias",
            &self.witness.bias,
            value_bits,
        )?;
        let mut accumulator = bias.signed;
        let mut accumulator_witness = self.witness.bias.signed();
        let mut accumulator_radius = bias.radius;
        let mut radius_witness = self.witness.bias.radius;

        for (index, (left_witness, right_witness)) in self.witness.terms.iter().enumerate() {
            let left = allocate_signed(
                cs.namespace(|| format!("term_{index}_left")),
                "term_left",
                left_witness,
                value_bits,
            )?;
            let right = allocate_signed(
                cs.namespace(|| format!("term_{index}_right")),
                "term_right",
                right_witness,
                value_bits,
            )?;
            let product_abs = left.magnitude.mul(
                cs.namespace(|| format!("term_{index}_abs_product")),
                &right.magnitude,
            )?;
            let raw =
                BigUint::from(left_witness.magnitude) * BigUint::from(right_witness.magnitude);
            let floor_witness = (&raw / &scale_big)
                .to_u128()
                .expect("produto arredondado cabe em u128");
            let remainder_witness = (&raw % &scale_big)
                .to_u128()
                .expect("resto do produto cabe em u128");
            let (floor, floor_bits) = allocate_ranged(
                cs.namespace(|| format!("term_{index}_floor")),
                "product_floor",
                floor_witness,
                value_bits + 8,
            )?;
            let (remainder, remainder_bits) = allocate_ranged(
                cs.namespace(|| format!("term_{index}_remainder")),
                "product_remainder",
                remainder_witness,
                bits,
            )?;
            cs.enforce(
                || format!("term_{index} product division"),
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + product_abs.get_variable()
                        - (scale, floor.get_variable())
                        - remainder.get_variable()
                },
            );
            let rounded_up_witness = remainder_witness > scale_u128 / 2
                || (remainder_witness == scale_u128 / 2 && floor_witness & 1 == 1);
            let rounded_up = allocate_boolean_num(
                cs.namespace(|| format!("term_{index}_round_up")),
                "product_round_up",
                rounded_up_witness,
            )?;
            enforce_constant_divisor_rounding(
                cs.namespace(|| format!("term_{index}_rounding")),
                ConstantDivisorRoundingArgs {
                    mode: CircuitRounding::NearestTiesToEven,
                    floor_witness,
                    remainder_witness,
                    divisor: scale_u128,
                    floor_bits: &floor_bits,
                    remainder: &remainder,
                    round_up: &rounded_up,
                },
            )?;
            let rounded_witness = floor_witness + u128::from(rounded_up_witness);
            let (rounded, _) = allocate_ranged(
                cs.namespace(|| format!("term_{index}_rounded")),
                "product_rounded",
                rounded_witness,
                value_bits + 8,
            )?;
            cs.enforce(
                || format!("term_{index} rounded product"),
                |lc| lc,
                |lc| lc,
                |lc| lc + floor.get_variable() + rounded_up.get_variable() - rounded.get_variable(),
            );
            let product_negative_witness = left_witness.negative ^ right_witness.negative;
            let product_negative = allocate_boolean_num(
                cs.namespace(|| format!("term_{index}_product_negative")),
                "product_negative",
                product_negative_witness,
            )?;
            cs.enforce(
                || format!("term_{index} sign xor"),
                |lc| lc + left.negative.get_variable(),
                |lc| lc + (F::from(2), right.negative.get_variable()),
                |lc| {
                    lc + left.negative.get_variable() + right.negative.get_variable()
                        - product_negative.get_variable()
                },
            );
            let product_signed_witness = if product_negative_witness {
                -(rounded_witness as i128)
            } else {
                rounded_witness as i128
            };
            let product_signed = AllocatedNum::alloc(
                cs.namespace(|| format!("term_{index}_signed_product")),
                || Ok(field_from_i128(product_signed_witness)),
            )?;
            cs.enforce(
                || format!("term_{index} signed product"),
                |lc| lc + rounded.get_variable(),
                |lc| lc + CS::one() - (F::from(2), product_negative.get_variable()),
                |lc| lc + product_signed.get_variable(),
            );

            let left_error = left.magnitude.mul(
                cs.namespace(|| format!("term_{index}_left_error")),
                &right.radius,
            )?;
            let right_error = right.magnitude.mul(
                cs.namespace(|| format!("term_{index}_right_error")),
                &left.radius,
            )?;
            let cross_error = left.radius.mul(
                cs.namespace(|| format!("term_{index}_cross_error")),
                &right.radius,
            )?;
            let error_numerator = BigUint::from(left_witness.magnitude)
                * BigUint::from(right_witness.radius)
                + BigUint::from(right_witness.magnitude) * BigUint::from(left_witness.radius)
                + BigUint::from(left_witness.radius) * BigUint::from(right_witness.radius);
            let error_floor_witness = (&error_numerator / &scale_big)
                .to_u128()
                .expect("erro propagado cabe em u128");
            let error_remainder_witness = (&error_numerator % &scale_big)
                .to_u128()
                .expect("resto do erro cabe em u128");
            let (error_floor, _) = allocate_ranged(
                cs.namespace(|| format!("term_{index}_error_floor")),
                "error_floor",
                error_floor_witness,
                value_bits + 8,
            )?;
            let (error_remainder, error_remainder_bits) = allocate_ranged(
                cs.namespace(|| format!("term_{index}_error_remainder")),
                "error_remainder",
                error_remainder_witness,
                bits,
            )?;
            cs.enforce(
                || format!("term_{index} error division"),
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + left_error.get_variable()
                        + right_error.get_variable()
                        + cross_error.get_variable()
                        - (scale, error_floor.get_variable())
                        - error_remainder.get_variable()
                },
            );
            let error_nonzero = allocate_nonzero_indicator_from_bits(
                cs.namespace(|| format!("term_{index}_error_nonzero")),
                &error_remainder_bits,
            )?;
            let center_nonzero = allocate_nonzero_indicator_from_bits(
                cs.namespace(|| format!("term_{index}_center_nonzero")),
                &remainder_bits,
            )?;
            let product_radius_witness = error_floor_witness
                + u128::from(error_remainder_witness != 0)
                + u128::from(remainder_witness != 0);
            let (product_radius, _) = allocate_ranged(
                cs.namespace(|| format!("term_{index}_product_radius")),
                "product_radius",
                product_radius_witness,
                value_bits + 8,
            )?;
            cs.enforce(
                || format!("term_{index} product radius"),
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + error_floor.get_variable()
                        + error_nonzero.get_variable()
                        + center_nonzero.get_variable()
                        - product_radius.get_variable()
                },
            );

            accumulator_witness += product_signed_witness;
            let offset = 64_u128 * scale_u128;
            let accumulator_offset_witness = (accumulator_witness + offset as i128) as u128;
            let (accumulator_offset, _) = allocate_ranged(
                cs.namespace(|| format!("term_{index}_accumulator_offset")),
                "accumulator_offset",
                accumulator_offset_witness,
                bits + 8,
            )?;
            let next_accumulator =
                AllocatedNum::alloc(cs.namespace(|| format!("term_{index}_accumulator")), || {
                    Ok(field_from_i128(accumulator_witness))
                })?;
            cs.enforce(
                || format!("term_{index} accumulator offset"),
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + next_accumulator.get_variable() + (field_from_u128::<F>(offset), CS::one())
                        - accumulator_offset.get_variable()
                },
            );
            cs.enforce(
                || format!("term_{index} accumulate center"),
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + accumulator.get_variable() + product_signed.get_variable()
                        - next_accumulator.get_variable()
                },
            );
            radius_witness += product_radius_witness;
            let (next_radius, _) = allocate_ranged(
                cs.namespace(|| format!("term_{index}_accumulator_radius")),
                "accumulator_radius",
                radius_witness,
                bits + 8,
            )?;
            cs.enforce(
                || format!("term_{index} accumulate radius"),
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + accumulator_radius.get_variable() + product_radius.get_variable()
                        - next_radius.get_variable()
                },
            );
            accumulator = next_accumulator;
            accumulator_radius = next_radius;
        }

        let offset = 64_u128 * scale_u128;
        let (output_center_offset, _) = allocate_ranged(
            cs.namespace(|| "score_output_center_offset"),
            "score_output_center_offset",
            self.witness.output_center_offset,
            bits + 8,
        )?;
        cs.enforce(
            || "score output center",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + accumulator.get_variable() + (field_from_u128::<F>(offset), CS::one())
                    - output_center_offset.get_variable()
            },
        );
        cs.enforce(
            || "score output radius",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + accumulator_radius.get_variable()
                    - (field_from_u128::<F>(self.witness.output_radius), CS::one())
            },
        );
        let decision = enforce_certified_decision(
            cs.namespace(|| "score_decision"),
            "score_decision",
            CertifiedDecisionArgs {
                center: &output_center_offset,
                center_witness: self.witness.output_center_offset,
                radius: &accumulator_radius,
                radius_witness: self.witness.output_radius,
                threshold: self.witness.threshold_offset(),
                decision_witness: self.witness.decision,
                comparison_bits: bits + 9,
            },
        )?;
        let next_iteration = AllocatedNum::alloc(cs.namespace(|| "score_next_iteration"), || {
            z[0].get_value()
                .map(|value| value + F::ONE)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "score increment iteration",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );
        Ok(vec![next_iteration, decision, z[2].clone()])
    }
}

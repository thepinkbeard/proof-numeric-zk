//! FP32 positivo-normal exato em R1CS.
//!
//! O gadget não implementa classes especiais. `+0` tem a codificação
//! `(expoente=0, significando=0, is_zero=1)`; todo valor não nulo usa o
//! significando explícito de 24 bits e expoente enviesado em `1..=254`.

use ff::PrimeField;
use nova_snark::{
    frontend::{
        num::AllocatedNum, AllocatedBit, ConstraintSystem, LinearCombination, SynthesisError,
    },
    traits::circuit::StepCircuit,
};
use num_bigint::BigUint;
use numeric_reference::{
    restricted_logistic_step, restricted_stable_step, RestrictedFp32, RestrictedFp32Error,
};

const SIGNIFICAND_BITS: usize = 24;
const HIDDEN_BIT: u128 = 1 << 23;
const ONE_SCALED_EXPONENT: usize = 149;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CircuitFp32 {
    pub exponent: u8,
    pub significand: u32,
    pub is_zero: bool,
}

impl CircuitFp32 {
    pub fn from_restricted(value: RestrictedFp32) -> Self {
        Self {
            exponent: value.exponent(),
            significand: value.significand(),
            is_zero: value.is_zero(),
        }
    }

    pub fn from_bits(bits: u32) -> Result<Self, RestrictedFp32Error> {
        RestrictedFp32::from_bits(bits).map(Self::from_restricted)
    }

    pub fn bits(self) -> u32 {
        if self.is_zero {
            0
        } else {
            ((self.exponent as u32) << 23) | (self.significand & 0x7f_ffff)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MulTrace {
    raw_product: u64,
    high_product: bool,
    floor: u32,
    remainder: u32,
    round_up: bool,
    base_exponent: u8,
    rounded: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fp32MulWitness {
    pub left: CircuitFp32,
    pub right: CircuitFp32,
    pub output: CircuitFp32,
    trace: MulTrace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AddTrace {
    exact_sum: BigUint,
    base_exponent: u8,
    divisor: u128,
    floor: u32,
    remainder: u128,
    rounded: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fp32AddWitness {
    pub left: CircuitFp32,
    pub right: CircuitFp32,
    pub output: CircuitFp32,
    trace: AddTrace,
}

impl Fp32AddWitness {
    pub fn compute(left_bits: u32, right_bits: u32) -> Result<Self, RestrictedFp32Error> {
        let left_value = RestrictedFp32::from_bits(left_bits)?;
        let right_value = RestrictedFp32::from_bits(right_bits)?;
        let output_value = RestrictedFp32::from_f32(left_value.value() + right_value.value())?;
        let left = CircuitFp32::from_restricted(left_value);
        let right = CircuitFp32::from_restricted(right_value);
        let output = CircuitFp32::from_restricted(output_value);
        let trace = add_trace(left, right, output);
        Ok(Self {
            left,
            right,
            output,
            trace,
        })
    }
}

fn add_trace(left: CircuitFp32, right: CircuitFp32, output: CircuitFp32) -> AddTrace {
    let exact_sum = scaled_integer(left) + scaled_integer(right);
    if exact_sum == BigUint::from(0_u8) {
        return AddTrace {
            exact_sum,
            base_exponent: 0,
            divisor: 1,
            floor: 0,
            remainder: 0,
            rounded: 0,
        };
    }
    let bit_length = exact_sum.bits() as usize;
    let shift = bit_length.saturating_sub(24);
    let divisor = 1_u128 << shift;
    let floor = (&exact_sum >> shift).try_into().expect("floor de 24 bits");
    let remainder = (&exact_sum % BigUint::from(divisor))
        .try_into()
        .expect("resto da soma cabe em u128");
    let twice = remainder * 2;
    let rounded = floor + u32::from(twice > divisor || (twice == divisor && floor & 1 == 1));
    let carry = rounded == (1 << 24);
    let base_exponent = (shift + 1) as u8;
    assert_eq!(output.exponent, base_exponent + u8::from(carry));
    assert_eq!(output.significand, if carry { 1 << 23 } else { rounded });
    AddTrace {
        exact_sum,
        base_exponent,
        divisor,
        floor,
        remainder,
        rounded,
    }
}

fn scaled_integer(value: CircuitFp32) -> BigUint {
    if value.is_zero {
        BigUint::from(0_u8)
    } else {
        BigUint::from(value.significand) << (value.exponent as usize - 1)
    }
}

impl Fp32MulWitness {
    pub fn compute(left_bits: u32, right_bits: u32) -> Result<Self, RestrictedFp32Error> {
        let left_value = RestrictedFp32::from_bits(left_bits)?;
        let right_value = RestrictedFp32::from_bits(right_bits)?;
        let output_value = RestrictedFp32::from_f32(left_value.value() * right_value.value())?;
        let left = CircuitFp32::from_restricted(left_value);
        let right = CircuitFp32::from_restricted(right_value);
        let output = CircuitFp32::from_restricted(output_value);
        let trace = mul_trace(left, right, output);
        Ok(Self {
            left,
            right,
            output,
            trace,
        })
    }
}

fn mul_trace(left: CircuitFp32, right: CircuitFp32, output: CircuitFp32) -> MulTrace {
    if left.is_zero || right.is_zero {
        assert!(output.is_zero);
        return MulTrace {
            raw_product: 0,
            high_product: false,
            floor: 0,
            remainder: 0,
            round_up: false,
            base_exponent: 0,
            rounded: 0,
        };
    }
    let raw_product = left.significand as u64 * right.significand as u64;
    let high_product = raw_product >= (1_u64 << 47);
    let shift = 23 + usize::from(high_product);
    let divisor = 1_u64 << shift;
    let floor = (raw_product / divisor) as u32;
    let remainder = (raw_product % divisor) as u32;
    let twice = (remainder as u64) * 2;
    let round_up = twice > divisor || (twice == divisor && floor & 1 == 1);
    let rounded = floor + u32::from(round_up);
    let base_exponent_i16 =
        left.exponent as i16 + right.exponent as i16 - 127 + i16::from(high_product);
    assert!(
        (1..=254).contains(&base_exponent_i16),
        "underflow/overflow fora do perfil FP32 restrito"
    );
    let base_exponent = base_exponent_i16 as u8;
    let carry = rounded == (1 << 24);
    assert_eq!(
        output.exponent,
        base_exponent + u8::from(carry),
        "expoente FP32 divergente"
    );
    assert_eq!(
        output.significand,
        rounded - u32::from(carry) * (1 << 23),
        "significando FP32 divergente"
    );
    MulTrace {
        raw_product,
        high_product,
        floor,
        remainder,
        round_up,
        base_exponent,
        rounded,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SubOneTrace {
    input_scaled: BigUint,
    difference: BigUint,
    base_exponent: u8,
    divisor: u128,
    floor: u32,
    remainder: u128,
    round_up: bool,
    rounded: u32,
    input_is_one: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fp32SubOneWitness {
    pub input: CircuitFp32,
    pub output: CircuitFp32,
    trace: SubOneTrace,
}

impl Fp32SubOneWitness {
    pub fn compute(input_bits: u32) -> Result<Self, RestrictedFp32Error> {
        let input_value = RestrictedFp32::from_bits(input_bits)?;
        if !(0.0..=1.0).contains(&input_value.value()) {
            return Err(RestrictedFp32Error::OutsideDomain);
        }
        let output_value = RestrictedFp32::from_f32(1.0_f32 - input_value.value())?;
        let input = CircuitFp32::from_restricted(input_value);
        let output = CircuitFp32::from_restricted(output_value);
        let trace = sub_one_trace(input, output);
        Ok(Self {
            input,
            output,
            trace,
        })
    }
}

fn sub_one_trace(input: CircuitFp32, output: CircuitFp32) -> SubOneTrace {
    let one_scaled = BigUint::from(1_u8) << ONE_SCALED_EXPONENT;
    let input_scaled = if input.is_zero {
        BigUint::from(0_u8)
    } else {
        BigUint::from(input.significand) << (input.exponent as usize - 1)
    };
    let difference = &one_scaled - &input_scaled;
    let input_is_one = difference == BigUint::from(0_u8);
    if input_is_one {
        assert!(output.is_zero);
        return SubOneTrace {
            input_scaled,
            difference,
            base_exponent: 0,
            divisor: 1,
            floor: 0,
            remainder: 0,
            round_up: false,
            rounded: 0,
            input_is_one,
        };
    }

    let highest_bit = difference.bits() - 1;
    let base_exponent: u8 = (highest_bit - 22)
        .try_into()
        .expect("resultado normal da subtração tem expoente FP32");
    let divisor = 1_u128 << (base_exponent - 1);
    let floor: u32 = (&difference / BigUint::from(divisor))
        .try_into()
        .expect("quociente normalizado cabe em 24 bits");
    let remainder: u128 = (&difference % BigUint::from(divisor))
        .try_into()
        .expect("resto da subtração cabe em u128");
    let twice = remainder * 2;
    let round_up = twice > divisor || (twice == divisor && floor & 1 == 1);
    let rounded = floor + u32::from(round_up);
    let carry = rounded == (1 << 24);
    assert_eq!(output.exponent, base_exponent + u8::from(carry));
    assert_eq!(
        output.significand,
        rounded - u32::from(carry) * HIDDEN_BIT as u32
    );

    SubOneTrace {
        input_scaled,
        difference,
        base_exponent,
        divisor,
        floor,
        remainder,
        round_up,
        rounded,
        input_is_one,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestrictedFp32StepWitness {
    pub input: CircuitFp32,
    pub complement: CircuitFp32,
    pub product: CircuitFp32,
    pub parameter: CircuitFp32,
    pub output: CircuitFp32,
    pub decision_half: bool,
    sub_one: Fp32SubOneWitness,
    first_mul: Fp32MulWitness,
    second_mul: Fp32MulWitness,
}

/// Expoente enviesado do limiar `1/2` no perfil FP32 restrito.
pub const HALF_BIASED_EXPONENT: u8 = 126;
/// Expoente enviesado do limiar `1` no perfil FP32 restrito.
pub const ONE_BIASED_EXPONENT: u8 = 127;

/// Converte um limiar potência de dois em seu expoente enviesado binary32.
///
/// Retorna `None` quando o limiar não é uma potência de dois positiva normal;
/// o gadget de decisão FP32 compara expoentes e só é exato nesse caso.
pub fn power_of_two_biased_exponent(threshold: f32) -> Option<u8> {
    if !threshold.is_finite() || threshold <= 0.0 {
        return None;
    }
    let bits = threshold.to_bits();
    if bits & 0x7f_ffff != 0 {
        return None;
    }
    let biased = ((bits >> 23) & 0xff) as u8;
    if biased == 0 || biased == 0xff {
        return None;
    }
    Some(biased)
}

impl RestrictedFp32StepWitness {
    pub fn compute(input_bits: u32, parameter_bits: u32) -> Result<Self, RestrictedFp32Error> {
        Self::compute_with_threshold(input_bits, parameter_bits, HALF_BIASED_EXPONENT)
    }

    /// Variante do protocolo de Pareto: o limiar é um parâmetro público,
    /// expresso pelo expoente enviesado de uma potência de dois.
    pub fn compute_with_threshold(
        input_bits: u32,
        parameter_bits: u32,
        threshold_biased: u8,
    ) -> Result<Self, RestrictedFp32Error> {
        let reference = restricted_logistic_step(input_bits, parameter_bits)?;
        let sub_one = Fp32SubOneWitness::compute(input_bits)?;
        let first_mul = Fp32MulWitness::compute(input_bits, reference.complement.bits())?;
        let second_mul = Fp32MulWitness::compute(reference.product.bits(), parameter_bits)?;
        assert_eq!(sub_one.output.bits(), reference.complement.bits());
        assert_eq!(first_mul.output.bits(), reference.product.bits());
        assert_eq!(second_mul.output.bits(), reference.output.bits());
        Ok(Self {
            input: CircuitFp32::from_restricted(reference.input),
            complement: CircuitFp32::from_restricted(reference.complement),
            product: CircuitFp32::from_restricted(reference.product),
            parameter: CircuitFp32::from_restricted(reference.parameter),
            output: CircuitFp32::from_restricted(reference.output),
            decision_half: reference.output.exponent() >= threshold_biased
                && !reference.output.is_zero(),
            sub_one,
            first_mul,
            second_mul,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestrictedFp32StableStepWitness {
    pub input: CircuitFp32,
    pub alpha: CircuitFp32,
    pub scaled: CircuitFp32,
    pub innovation: CircuitFp32,
    pub output: CircuitFp32,
    pub decision_one: bool,
    multiplication: Fp32MulWitness,
    addition: Fp32AddWitness,
}

impl RestrictedFp32StableStepWitness {
    pub fn compute(
        input_bits: u32,
        alpha_bits: u32,
        innovation_bits: u32,
    ) -> Result<Self, RestrictedFp32Error> {
        Self::compute_with_threshold(input_bits, alpha_bits, innovation_bits, ONE_BIASED_EXPONENT)
    }

    /// Variante do protocolo de Pareto: o limiar é um parâmetro público,
    /// expresso pelo expoente enviesado de uma potência de dois.
    pub fn compute_with_threshold(
        input_bits: u32,
        alpha_bits: u32,
        innovation_bits: u32,
        threshold_biased: u8,
    ) -> Result<Self, RestrictedFp32Error> {
        let reference = restricted_stable_step(input_bits, alpha_bits, innovation_bits)?;
        let multiplication = Fp32MulWitness::compute(input_bits, alpha_bits)?;
        let addition = Fp32AddWitness::compute(reference.scaled.bits(), innovation_bits)?;
        assert_eq!(multiplication.output.bits(), reference.scaled.bits());
        assert_eq!(addition.output.bits(), reference.output.bits());
        Ok(Self {
            input: CircuitFp32::from_restricted(reference.input),
            alpha: CircuitFp32::from_restricted(reference.alpha),
            scaled: CircuitFp32::from_restricted(reference.scaled),
            innovation: CircuitFp32::from_restricted(reference.innovation),
            output: CircuitFp32::from_restricted(reference.output),
            decision_one: reference.output.exponent() >= threshold_biased
                && !reference.output.is_zero(),
            multiplication,
            addition,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Fp32MulCircuit {
    pub witness: Fp32MulWitness,
}

#[derive(Clone, Debug)]
pub struct Fp32AddCircuit {
    pub witness: Fp32AddWitness,
}

impl Fp32AddCircuit {
    pub const fn arity_value() -> usize {
        7
    }
}

impl Fp32MulCircuit {
    pub const fn arity_value() -> usize {
        7 // iteration, lhs(exp,sig,zero), rhs(exp,sig,zero)
    }

    pub const fn range_check_constraints() -> usize {
        417
    }
}

#[derive(Clone, Debug)]
pub struct Fp32SubOneCircuit {
    pub witness: Fp32SubOneWitness,
}

impl Fp32SubOneCircuit {
    pub const fn arity_value() -> usize {
        4 // iteration, x(exp,sig,zero)
    }

    pub const fn range_check_constraints() -> usize {
        855
    }
}

#[derive(Clone, Debug)]
pub struct RestrictedFp32StepCircuit {
    pub witness: RestrictedFp32StepWitness,
    pub parameter_bits: u32,
    /// Expoente enviesado do limiar público, necessariamente potência de dois.
    pub threshold_biased_exponent: u8,
}

#[derive(Clone, Debug)]
pub struct RestrictedFp32StableStepCircuit {
    pub witness: RestrictedFp32StableStepWitness,
    pub alpha_bits: u32,
    /// Expoente enviesado do limiar público, necessariamente potência de dois.
    pub threshold_biased_exponent: u8,
    pub innovation_bits: u32,
}

impl RestrictedFp32StableStepCircuit {
    pub const fn arity_value() -> usize {
        5
    }
}

impl RestrictedFp32StepCircuit {
    pub const fn arity_value() -> usize {
        5 // iteration, x(exp,sig,zero), decision(x>=1/2)
    }

    /// Bits booleanos mais uma reconstrução por decomposição.
    pub const fn range_check_constraints() -> usize {
        1_551
    }
}

#[derive(Clone)]
struct AllocatedFp32<F: PrimeField> {
    exponent: AllocatedNum<F>,
    exponent_minus_one_bits: Vec<AllocatedBit>,
    significand: AllocatedNum<F>,
    is_zero: AllocatedNum<F>,
}

fn field_from_u128<F: PrimeField>(value: u128) -> F {
    let high = F::from((value >> 64) as u64);
    let mut two64 = F::ONE;
    for _ in 0..64 {
        two64 = two64.double();
    }
    high * two64 + F::from(value as u64)
}

fn field_from_biguint<F: PrimeField>(value: &BigUint) -> F {
    let mut result = F::ZERO;
    let mut coefficient = F::ONE;
    for limb in value.to_u64_digits() {
        result += coefficient * F::from(limb);
        for _ in 0..64 {
            coefficient = coefficient.double();
        }
    }
    result
}

fn allocate_boolean<F, CS>(
    mut cs: CS,
    name: &'static str,
    value: bool,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let number = AllocatedNum::alloc(cs.namespace(|| name), || {
        Ok(if value { F::ONE } else { F::ZERO })
    })?;
    cs.enforce(
        || format!("{name} boolean"),
        |lc| lc + number.get_variable(),
        |lc| lc + CS::one() - number.get_variable(),
        |lc| lc,
    );
    Ok(number)
}

fn range_check<F, CS>(
    mut cs: CS,
    number: &AllocatedNum<F>,
    witness: u128,
    bits: usize,
) -> Result<Vec<AllocatedBit>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    assert!(bits <= 128);
    let allocated = (0..bits)
        .map(|index| {
            AllocatedBit::alloc(
                cs.namespace(|| format!("bit_{index}")),
                Some(((witness >> index) & 1) == 1),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut reconstruction = LinearCombination::zero();
    let mut coefficient = F::ONE;
    for bit in &allocated {
        reconstruction = reconstruction + (coefficient, bit.get_variable());
        coefficient = coefficient.double();
    }
    cs.enforce(
        || "range reconstruction",
        |lc| lc,
        |lc| lc,
        |_| reconstruction - number.get_variable(),
    );
    Ok(allocated)
}

fn allocate_ranged<F, CS>(
    mut cs: CS,
    name: &'static str,
    witness: u128,
    bits: usize,
) -> Result<(AllocatedNum<F>, Vec<AllocatedBit>), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let number = AllocatedNum::alloc(cs.namespace(|| name), || Ok(field_from_u128(witness)))?;
    let bits = range_check(
        cs.namespace(|| format!("{name} range")),
        &number,
        witness,
        bits,
    )?;
    Ok((number, bits))
}

fn constrain_fp32<F, CS>(
    mut cs: CS,
    exponent: AllocatedNum<F>,
    significand: AllocatedNum<F>,
    is_zero: AllocatedNum<F>,
    witness: CircuitFp32,
) -> Result<AllocatedFp32<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let _exponent_bits = range_check(
        cs.namespace(|| "exponent range"),
        &exponent,
        witness.exponent as u128,
        8,
    )?;
    let significand_bits = range_check(
        cs.namespace(|| "significand range"),
        &significand,
        witness.significand as u128,
        SIGNIFICAND_BITS,
    )?;
    cs.enforce(
        || "zero flag boolean",
        |lc| lc + is_zero.get_variable(),
        |lc| lc + CS::one() - is_zero.get_variable(),
        |lc| lc,
    );
    cs.enforce(
        || "zero has exponent zero",
        |lc| lc + is_zero.get_variable(),
        |lc| lc + exponent.get_variable(),
        |lc| lc,
    );
    cs.enforce(
        || "zero has significand zero",
        |lc| lc + is_zero.get_variable(),
        |lc| lc + significand.get_variable(),
        |lc| lc,
    );
    let high_bit = significand_bits
        .last()
        .ok_or_else(|| SynthesisError::Unsatisfiable("missing significand high bit".to_owned()))?;
    cs.enforce(
        || "normal has explicit hidden bit",
        |lc| lc,
        |lc| lc,
        |lc| lc + high_bit.get_variable() + is_zero.get_variable() - CS::one(),
    );

    let exponent_minus_one_witness = if witness.is_zero {
        0
    } else {
        witness.exponent.saturating_sub(1)
    };
    let (exponent_minus_one, exponent_minus_one_bits) = allocate_ranged(
        cs.namespace(|| "exponent minus one"),
        "exponent_minus_one_value",
        exponent_minus_one_witness as u128,
        8,
    )?;
    cs.enforce(
        || "canonical exponent lower bound",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + exponent.get_variable() - exponent_minus_one.get_variable() - CS::one()
                + is_zero.get_variable()
        },
    );
    let exponent_gap_witness = 254_u16.saturating_sub(witness.exponent as u16) as u128;
    let (exponent_gap, _) = allocate_ranged(
        cs.namespace(|| "exponent upper gap"),
        "exponent_upper_gap_value",
        exponent_gap_witness,
        8,
    )?;
    cs.enforce(
        || "finite exponent upper bound",
        |lc| lc,
        |lc| lc,
        |lc| lc + exponent.get_variable() + exponent_gap.get_variable() - (F::from(254), CS::one()),
    );
    Ok(AllocatedFp32 {
        exponent,
        exponent_minus_one_bits,
        significand,
        is_zero,
    })
}

fn allocate_fp32<F, CS>(
    mut cs: CS,
    name: &'static str,
    witness: CircuitFp32,
) -> Result<AllocatedFp32<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let exponent = AllocatedNum::alloc(cs.namespace(|| format!("{name} exponent")), || {
        Ok(F::from(witness.exponent as u64))
    })?;
    let significand = AllocatedNum::alloc(cs.namespace(|| format!("{name} significand")), || {
        Ok(F::from(witness.significand as u64))
    })?;
    let is_zero = AllocatedNum::alloc(cs.namespace(|| format!("{name} zero")), || {
        Ok(if witness.is_zero { F::ONE } else { F::ZERO })
    })?;
    constrain_fp32(
        cs.namespace(|| format!("{name} canonical")),
        exponent,
        significand,
        is_zero,
        witness,
    )
}

fn pow2_from_bits<F, CS>(
    mut cs: CS,
    exponent_bits: &[AllocatedBit],
    exponent_witness: u8,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let mut accumulator_witness = 1_u128;
    let mut accumulator =
        AllocatedNum::alloc(cs.namespace(|| "pow2 accumulator 0"), || Ok(F::ONE))?;
    cs.enforce(
        || "pow2 accumulator starts at one",
        |lc| lc,
        |lc| lc,
        |lc| lc + accumulator.get_variable() - CS::one(),
    );
    for (index, bit) in exponent_bits.iter().take(7).enumerate() {
        let selected = ((exponent_witness >> index) & 1) == 1;
        let factor_power = 1_usize << index;
        let factor_witness = if selected { 1_u128 << factor_power } else { 1 };
        accumulator_witness *= factor_witness;
        let next = AllocatedNum::alloc(
            cs.namespace(|| format!("pow2 accumulator {}", index + 1)),
            || Ok(field_from_u128(accumulator_witness)),
        )?;
        let factor = field_from_u128::<F>(1_u128 << factor_power) - F::ONE;
        cs.enforce(
            || format!("pow2 conditional factor {index}"),
            |lc| lc + accumulator.get_variable(),
            |lc| lc + CS::one() + (factor, bit.get_variable()),
            |lc| lc + next.get_variable(),
        );
        accumulator = next;
    }
    Ok(accumulator)
}

struct RneArgs<'a, F: PrimeField> {
    floor_bits: &'a [AllocatedBit],
    floor_witness: u32,
    remainder: &'a AllocatedNum<F>,
    remainder_witness: u128,
    divisor: &'a AllocatedNum<F>,
    divisor_witness: u128,
    comparison_bits: usize,
}

fn enforce_rne<F, CS>(mut cs: CS, args: RneArgs<'_, F>) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let half_witness = args.divisor_witness / 2;
    let divisor_is_one_witness = args.divisor_witness == 1;
    let divisor_is_one = allocate_boolean(
        cs.namespace(|| "rne divisor is one"),
        "rne_divisor_is_one_value",
        divisor_is_one_witness,
    )?;
    let divisor_minus_one_witness = args.divisor_witness - 1;
    let divisor_minus_one_inverse =
        AllocatedNum::alloc(cs.namespace(|| "rne divisor minus one inverse"), || {
            if divisor_minus_one_witness == 0 {
                Ok(F::ZERO)
            } else {
                Option::<F>::from(field_from_u128::<F>(divisor_minus_one_witness).invert())
                    .ok_or_else(|| {
                        SynthesisError::Unsatisfiable("rne divisor minus one inverse".to_owned())
                    })
            }
        })?;
    cs.enforce(
        || "rne divisor is one indicator",
        |lc| lc + args.divisor.get_variable() - CS::one(),
        |lc| lc + divisor_minus_one_inverse.get_variable(),
        |lc| lc + CS::one() - divisor_is_one.get_variable(),
    );
    cs.enforce(
        || "rne divisor one zero branch",
        |lc| lc + args.divisor.get_variable() - CS::one(),
        |lc| lc + divisor_is_one.get_variable(),
        |lc| lc,
    );
    let (half, _) = allocate_ranged(
        cs.namespace(|| "rne half"),
        "rne_half_value",
        half_witness,
        args.comparison_bits,
    )?;
    cs.enforce(
        || "rne half relation",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + (F::from(2), half.get_variable()) + divisor_is_one.get_variable()
                - args.divisor.get_variable()
        },
    );

    let less_witness = args.remainder_witness < half_witness;
    let equal_witness = args.remainder_witness == half_witness;
    let greater_witness = args.remainder_witness > half_witness;
    let less = allocate_boolean(cs.namespace(|| "rne less"), "rne_less_value", less_witness)?;
    let equal = allocate_boolean(
        cs.namespace(|| "rne equal"),
        "rne_equal_value",
        equal_witness,
    )?;
    let greater = allocate_boolean(
        cs.namespace(|| "rne greater"),
        "rne_greater_value",
        greater_witness,
    )?;
    cs.enforce(
        || "rne comparison one hot",
        |lc| lc,
        |lc| lc,
        |lc| lc + less.get_variable() + equal.get_variable() + greater.get_variable() - CS::one(),
    );
    let less_diff_witness = if less_witness {
        half_witness - 1 - args.remainder_witness
    } else {
        0
    };
    let greater_diff_witness = if greater_witness {
        args.remainder_witness - half_witness - 1
    } else {
        0
    };
    let (less_diff, _) = allocate_ranged(
        cs.namespace(|| "rne less diff"),
        "rne_less_diff_value",
        less_diff_witness,
        args.comparison_bits,
    )?;
    let (greater_diff, _) = allocate_ranged(
        cs.namespace(|| "rne greater diff"),
        "rne_greater_diff_value",
        greater_diff_witness,
        args.comparison_bits,
    )?;
    cs.enforce(
        || "rne less relation",
        |lc| lc + less.get_variable(),
        |lc| {
            lc + half.get_variable()
                - CS::one()
                - args.remainder.get_variable()
                - less_diff.get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || "rne equal relation",
        |lc| lc + equal.get_variable(),
        |lc| lc + args.remainder.get_variable() - half.get_variable(),
        |lc| lc,
    );
    cs.enforce(
        || "rne greater relation",
        |lc| lc + greater.get_variable(),
        |lc| {
            lc + args.remainder.get_variable()
                - half.get_variable()
                - CS::one()
                - greater_diff.get_variable()
        },
        |lc| lc,
    );
    let floor_odd = args
        .floor_bits
        .first()
        .ok_or_else(|| SynthesisError::Unsatisfiable("missing floor parity".to_owned()))?;
    let tie_candidate_witness = equal_witness && args.floor_witness & 1 == 1;
    let tie_candidate = allocate_boolean(
        cs.namespace(|| "rne tie candidate"),
        "rne_tie_candidate_value",
        tie_candidate_witness,
    )?;
    cs.enforce(
        || "rne tie rounds iff floor odd",
        |lc| lc + equal.get_variable(),
        |lc| lc + floor_odd.get_variable(),
        |lc| lc + tie_candidate.get_variable(),
    );
    let tie_round_witness = tie_candidate_witness && !divisor_is_one_witness;
    let tie_round = allocate_boolean(
        cs.namespace(|| "rne tie round"),
        "rne_tie_round_value",
        tie_round_witness,
    )?;
    cs.enforce(
        || "unit divisor suppresses tie rounding",
        |lc| lc + tie_candidate.get_variable(),
        |lc| lc + CS::one() - divisor_is_one.get_variable(),
        |lc| lc + tie_round.get_variable(),
    );
    let round_up_witness = greater_witness || tie_round_witness;
    let round_up = allocate_boolean(
        cs.namespace(|| "rne round up"),
        "rne_round_up_value",
        round_up_witness,
    )?;
    cs.enforce(
        || "rne result",
        |lc| lc,
        |lc| lc,
        |lc| lc + greater.get_variable() + tie_round.get_variable() - round_up.get_variable(),
    );
    Ok(round_up)
}

fn synthesize_mul<F, CS>(
    mut cs: CS,
    left: &AllocatedFp32<F>,
    right: &AllocatedFp32<F>,
    witness: &Fp32MulWitness,
) -> Result<AllocatedFp32<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let output = allocate_fp32(cs.namespace(|| "mul output"), "mul_output", witness.output)?;
    let active_witness = !witness.left.is_zero && !witness.right.is_zero;
    let active = allocate_boolean(
        cs.namespace(|| "mul active"),
        "mul_active_value",
        active_witness,
    )?;
    cs.enforce(
        || "mul active iff both nonzero",
        |lc| lc + CS::one() - left.is_zero.get_variable(),
        |lc| lc + CS::one() - right.is_zero.get_variable(),
        |lc| lc + active.get_variable(),
    );
    cs.enforce(
        || "mul output zero iff inactive",
        |lc| lc,
        |lc| lc,
        |lc| lc + output.is_zero.get_variable() + active.get_variable() - CS::one(),
    );

    let raw_product = left
        .significand
        .mul(cs.namespace(|| "mul raw significands"), &right.significand)?;
    let raw_bits = range_check(
        cs.namespace(|| "mul raw range"),
        &raw_product,
        witness.trace.raw_product as u128,
        48,
    )?;
    let high_product = raw_bits
        .last()
        .ok_or_else(|| SynthesisError::Unsatisfiable("missing product high bit".to_owned()))?;
    let high_expected = F::from(witness.trace.high_product as u64);
    debug_assert_eq!(
        high_expected,
        if witness.trace.high_product {
            F::ONE
        } else {
            F::ZERO
        }
    );

    let divisor_witness = 1_u128 << (23 + usize::from(witness.trace.high_product));
    let divisor = AllocatedNum::alloc(cs.namespace(|| "mul divisor"), || {
        Ok(field_from_u128(divisor_witness))
    })?;
    let two23 = F::from(1_u64 << 23);
    cs.enforce(
        || "mul divisor selection",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + (two23, CS::one()) + (two23, high_product.get_variable()) - divisor.get_variable()
        },
    );
    let (floor, floor_bits) = allocate_ranged(
        cs.namespace(|| "mul floor"),
        "mul_floor_value",
        witness.trace.floor as u128,
        24,
    )?;
    cs.enforce(
        || "mul floor normalized iff active",
        |lc| lc,
        |lc| lc,
        |lc| lc + floor_bits[23].get_variable() - active.get_variable(),
    );
    let (remainder, remainder_bits) = allocate_ranged(
        cs.namespace(|| "mul remainder"),
        "mul_remainder_value",
        witness.trace.remainder as u128,
        24,
    )?;
    cs.enforce(
        || "mul low divisor has 23-bit remainder",
        |lc| lc + CS::one() - high_product.get_variable(),
        |lc| lc + remainder_bits[23].get_variable(),
        |lc| lc,
    );
    let floor_times_divisor = floor.mul(cs.namespace(|| "mul floor times divisor"), &divisor)?;
    cs.enforce(
        || "mul euclidean division",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + raw_product.get_variable()
                - floor_times_divisor.get_variable()
                - remainder.get_variable()
        },
    );

    let (base_exponent, _) = allocate_ranged(
        cs.namespace(|| "mul base exponent"),
        "mul_base_exponent_value",
        witness.trace.base_exponent as u128,
        8,
    )?;
    let exponent_relation_witness = witness.left.exponent as i16 + witness.right.exponent as i16
        - 127
        + i16::from(witness.trace.high_product)
        - witness.trace.base_exponent as i16;
    debug_assert!(!active_witness || exponent_relation_witness == 0);
    cs.enforce(
        || "mul biased exponent",
        |lc| lc + active.get_variable(),
        |lc| {
            lc + left.exponent.get_variable()
                + right.exponent.get_variable()
                + high_product.get_variable()
                - (F::from(127), CS::one())
                - base_exponent.get_variable()
        },
        |lc| lc,
    );
    let base_zero_witness = !active_witness;
    let base_zero = allocate_boolean(
        cs.namespace(|| "mul base zero"),
        "mul_base_zero_value",
        base_zero_witness,
    )?;
    cs.enforce(
        || "mul base zero matches inactive",
        |lc| lc,
        |lc| lc,
        |lc| lc + base_zero.get_variable() + active.get_variable() - CS::one(),
    );
    let dummy_significand =
        AllocatedNum::alloc(cs.namespace(|| "mul base dummy significand"), || {
            Ok(if active_witness {
                F::from(1_u64 << 23)
            } else {
                F::ZERO
            })
        })?;
    let base = constrain_fp32(
        cs.namespace(|| "mul base exponent canonical"),
        base_exponent,
        dummy_significand,
        base_zero,
        CircuitFp32 {
            exponent: witness.trace.base_exponent,
            significand: if active_witness { 1 << 23 } else { 0 },
            is_zero: !active_witness,
        },
    )?;

    let round_up = enforce_rne(
        cs.namespace(|| "mul rne"),
        RneArgs {
            floor_bits: &floor_bits,
            floor_witness: witness.trace.floor,
            remainder: &remainder,
            remainder_witness: witness.trace.remainder as u128,
            divisor: &divisor,
            divisor_witness,
            comparison_bits: 24,
        },
    )?;
    let (rounded, rounded_bits) = allocate_ranged(
        cs.namespace(|| "mul rounded"),
        "mul_rounded_value",
        witness.trace.rounded as u128,
        25,
    )?;
    cs.enforce(
        || "mul rounded value",
        |lc| lc,
        |lc| lc,
        |lc| lc + floor.get_variable() + round_up.get_variable() - rounded.get_variable(),
    );
    let carry = rounded_bits
        .last()
        .ok_or_else(|| SynthesisError::Unsatisfiable("missing rounding carry".to_owned()))?;
    cs.enforce(
        || "mul output exponent",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + base.exponent.get_variable() + carry.get_variable()
                - output.exponent.get_variable()
        },
    );
    cs.enforce(
        || "mul output significand",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + rounded.get_variable()
                - (F::from(1_u64 << 23), carry.get_variable())
                - output.significand.get_variable()
        },
    );
    Ok(output)
}

fn constrain_input_unit_interval<F, CS>(
    mut cs: CS,
    input: &AllocatedFp32<F>,
    witness: CircuitFp32,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let gap_witness = 127_u8.saturating_sub(witness.exponent) as u128;
    let (gap, gap_bits) = allocate_ranged(
        cs.namespace(|| "unit exponent gap"),
        "unit_exponent_gap_value",
        gap_witness,
        7,
    )?;
    cs.enforce(
        || "unit interval exponent upper bound",
        |lc| lc,
        |lc| lc,
        |lc| lc + input.exponent.get_variable() + gap.get_variable() - (F::from(127), CS::one()),
    );
    let gap_nonzero_witness = gap_witness != 0;
    let gap_nonzero = allocate_boolean(
        cs.namespace(|| "unit gap nonzero"),
        "unit_gap_nonzero_value",
        gap_nonzero_witness,
    )?;
    let gap_sum_witness = gap_bits
        .iter()
        .enumerate()
        .fold(0_u128, |sum, (index, bit)| {
            sum + u128::from(bit.get_value().unwrap_or(false)) * (1_u128 << index)
        });
    let inverse = AllocatedNum::alloc(cs.namespace(|| "unit gap inverse"), || {
        if gap_sum_witness == 0 {
            Ok(F::ZERO)
        } else {
            Option::<F>::from(field_from_u128::<F>(gap_sum_witness).invert())
                .ok_or_else(|| SynthesisError::Unsatisfiable("unit gap inverse".to_owned()))
        }
    })?;
    let mut gap_lc = LinearCombination::zero();
    let mut coefficient = F::ONE;
    for bit in &gap_bits {
        gap_lc = gap_lc + (coefficient, bit.get_variable());
        coefficient = coefficient.double();
    }
    cs.enforce(
        || "unit gap nonzero indicator",
        |_| gap_lc.clone(),
        |lc| lc + inverse.get_variable(),
        |lc| lc + gap_nonzero.get_variable(),
    );
    cs.enforce(
        || "unit gap zero branch",
        |_| gap_lc,
        |lc| lc + CS::one() - gap_nonzero.get_variable(),
        |lc| lc,
    );
    let exponent_is_127_witness = witness.exponent == 127;
    let exponent_is_127 = allocate_boolean(
        cs.namespace(|| "exponent is 127"),
        "exponent_is_127_value",
        exponent_is_127_witness,
    )?;
    cs.enforce(
        || "exponent 127 complement",
        |lc| lc,
        |lc| lc,
        |lc| lc + exponent_is_127.get_variable() + gap_nonzero.get_variable() - CS::one(),
    );
    cs.enforce(
        || "only one at exponent 127",
        |lc| lc + exponent_is_127.get_variable(),
        |lc| lc + input.significand.get_variable() - (F::from(1_u64 << 23), CS::one()),
        |lc| lc,
    );
    Ok(exponent_is_127)
}

fn synthesize_add<F, CS>(
    mut cs: CS,
    left: &AllocatedFp32<F>,
    right: &AllocatedFp32<F>,
    witness: &Fp32AddWitness,
) -> Result<AllocatedFp32<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let output = allocate_fp32(cs.namespace(|| "add output"), "add_output", witness.output)?;
    let left_power = pow2_from_bits(
        cs.namespace(|| "add left exponent power"),
        &left.exponent_minus_one_bits,
        if witness.left.is_zero {
            0
        } else {
            witness.left.exponent - 1
        },
    )?;
    let right_power = pow2_from_bits(
        cs.namespace(|| "add right exponent power"),
        &right.exponent_minus_one_bits,
        if witness.right.is_zero {
            0
        } else {
            witness.right.exponent - 1
        },
    )?;
    let left_scaled = left
        .significand
        .mul(cs.namespace(|| "add scaled left"), &left_power)?;
    let right_scaled = right
        .significand
        .mul(cs.namespace(|| "add scaled right"), &right_power)?;
    let exact_sum = AllocatedNum::alloc(cs.namespace(|| "add exact sum"), || {
        Ok(field_from_biguint(&witness.trace.exact_sum))
    })?;
    cs.enforce(
        || "add exact sum relation",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + left_scaled.get_variable() + right_scaled.get_variable() - exact_sum.get_variable()
        },
    );
    let base_zero = witness.output.is_zero;
    let base = allocate_fp32(
        cs.namespace(|| "add base exponent"),
        "add_base",
        CircuitFp32 {
            exponent: witness.trace.base_exponent,
            significand: if base_zero { 0 } else { 1 << 23 },
            is_zero: base_zero,
        },
    )?;
    let divisor = pow2_from_bits(
        cs.namespace(|| "add output ulp"),
        &base.exponent_minus_one_bits,
        if base_zero {
            0
        } else {
            witness.trace.base_exponent - 1
        },
    )?;
    let (floor, floor_bits) = allocate_ranged(
        cs.namespace(|| "add floor"),
        "add_floor_value",
        witness.trace.floor as u128,
        24,
    )?;
    cs.enforce(
        || "add floor normalized iff output nonzero",
        |lc| lc,
        |lc| lc,
        |lc| lc + floor_bits[23].get_variable() + output.is_zero.get_variable() - CS::one(),
    );
    let (remainder, _) = allocate_ranged(
        cs.namespace(|| "add remainder"),
        "add_remainder_value",
        witness.trace.remainder,
        127,
    )?;
    let remainder_gap_witness = witness.trace.divisor - 1 - witness.trace.remainder;
    let (remainder_gap, _) = allocate_ranged(
        cs.namespace(|| "add remainder gap"),
        "add_remainder_gap_value",
        remainder_gap_witness,
        127,
    )?;
    cs.enforce(
        || "add remainder below divisor",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + remainder.get_variable() + remainder_gap.get_variable() + CS::one()
                - divisor.get_variable()
        },
    );
    let floor_times_divisor = floor.mul(cs.namespace(|| "add floor times ulp"), &divisor)?;
    cs.enforce(
        || "add euclidean division",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + exact_sum.get_variable()
                - floor_times_divisor.get_variable()
                - remainder.get_variable()
        },
    );
    let round_up = enforce_rne(
        cs.namespace(|| "add rne"),
        RneArgs {
            floor_bits: &floor_bits,
            floor_witness: witness.trace.floor,
            remainder: &remainder,
            remainder_witness: witness.trace.remainder,
            divisor: &divisor,
            divisor_witness: witness.trace.divisor,
            comparison_bits: 127,
        },
    )?;
    let (rounded, rounded_bits) = allocate_ranged(
        cs.namespace(|| "add rounded"),
        "add_rounded_value",
        witness.trace.rounded as u128,
        25,
    )?;
    cs.enforce(
        || "add rounded value",
        |lc| lc,
        |lc| lc,
        |lc| lc + floor.get_variable() + round_up.get_variable() - rounded.get_variable(),
    );
    let carry = rounded_bits
        .last()
        .ok_or_else(|| SynthesisError::Unsatisfiable("missing addition carry".to_owned()))?;
    cs.enforce(
        || "add output exponent",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + base.exponent.get_variable() + carry.get_variable()
                - output.exponent.get_variable()
        },
    );
    cs.enforce(
        || "add output significand",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + rounded.get_variable()
                - (F::from(1_u64 << 23), carry.get_variable())
                - output.significand.get_variable()
        },
    );
    Ok(output)
}

fn synthesize_sub_one<F, CS>(
    mut cs: CS,
    input: &AllocatedFp32<F>,
    witness: &Fp32SubOneWitness,
) -> Result<AllocatedFp32<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let input_is_one =
        constrain_input_unit_interval(cs.namespace(|| "sub input domain"), input, witness.input)?;
    debug_assert_eq!(witness.trace.input_is_one, witness.output.is_zero);
    let output = allocate_fp32(cs.namespace(|| "sub output"), "sub_output", witness.output)?;
    cs.enforce(
        || "sub output zero iff input one",
        |lc| lc,
        |lc| lc,
        |lc| lc + output.is_zero.get_variable() - input_is_one.get_variable(),
    );

    let input_power = pow2_from_bits(
        cs.namespace(|| "sub input exponent power"),
        &input.exponent_minus_one_bits,
        if witness.input.is_zero {
            0
        } else {
            witness.input.exponent - 1
        },
    )?;
    let input_scaled = input
        .significand
        .mul(cs.namespace(|| "sub scaled input"), &input_power)?;
    let difference = AllocatedNum::alloc(cs.namespace(|| "sub exact difference"), || {
        Ok(field_from_biguint(&witness.trace.difference))
    })?;
    let one_scaled = {
        let mut value = F::ONE;
        for _ in 0..ONE_SCALED_EXPONENT {
            value = value.double();
        }
        value
    };
    cs.enforce(
        || "sub exact difference relation",
        |lc| lc,
        |lc| lc,
        |lc| lc + (one_scaled, CS::one()) - input_scaled.get_variable() - difference.get_variable(),
    );

    let base_zero_witness = witness.trace.input_is_one;
    let base = allocate_fp32(
        cs.namespace(|| "sub base exponent"),
        "sub_base",
        CircuitFp32 {
            exponent: witness.trace.base_exponent,
            significand: if base_zero_witness { 0 } else { 1 << 23 },
            is_zero: base_zero_witness,
        },
    )?;
    let divisor = pow2_from_bits(
        cs.namespace(|| "sub output ulp"),
        &base.exponent_minus_one_bits,
        if base_zero_witness {
            0
        } else {
            witness.trace.base_exponent - 1
        },
    )?;
    debug_assert_eq!(
        divisor.get_value(),
        Some(field_from_u128(witness.trace.divisor))
    );
    let (floor, floor_bits) = allocate_ranged(
        cs.namespace(|| "sub floor"),
        "sub_floor_value",
        witness.trace.floor as u128,
        24,
    )?;
    cs.enforce(
        || "sub floor normalized iff output nonzero",
        |lc| lc,
        |lc| lc,
        |lc| lc + floor_bits[23].get_variable() + output.is_zero.get_variable() - CS::one(),
    );
    let (remainder, _) = allocate_ranged(
        cs.namespace(|| "sub remainder"),
        "sub_remainder_value",
        witness.trace.remainder,
        127,
    )?;
    let remainder_gap_witness = witness.trace.divisor - 1 - witness.trace.remainder;
    let (remainder_gap, _) = allocate_ranged(
        cs.namespace(|| "sub remainder gap"),
        "sub_remainder_gap_value",
        remainder_gap_witness,
        127,
    )?;
    cs.enforce(
        || "sub remainder below divisor",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + remainder.get_variable() + remainder_gap.get_variable() + CS::one()
                - divisor.get_variable()
        },
    );
    let floor_times_divisor = floor.mul(cs.namespace(|| "sub floor times ulp"), &divisor)?;
    cs.enforce(
        || "sub euclidean division",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + difference.get_variable()
                - floor_times_divisor.get_variable()
                - remainder.get_variable()
        },
    );
    let round_up = enforce_rne(
        cs.namespace(|| "sub rne"),
        RneArgs {
            floor_bits: &floor_bits,
            floor_witness: witness.trace.floor,
            remainder: &remainder,
            remainder_witness: witness.trace.remainder,
            divisor: &divisor,
            divisor_witness: witness.trace.divisor,
            comparison_bits: 127,
        },
    )?;
    let (rounded, rounded_bits) = allocate_ranged(
        cs.namespace(|| "sub rounded"),
        "sub_rounded_value",
        witness.trace.rounded as u128,
        25,
    )?;
    cs.enforce(
        || "sub rounded value",
        |lc| lc,
        |lc| lc,
        |lc| lc + floor.get_variable() + round_up.get_variable() - rounded.get_variable(),
    );
    let carry = rounded_bits
        .last()
        .ok_or_else(|| SynthesisError::Unsatisfiable("missing subtraction carry".to_owned()))?;
    cs.enforce(
        || "sub output exponent",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + base.exponent.get_variable() + carry.get_variable()
                - output.exponent.get_variable()
        },
    );
    cs.enforce(
        || "sub output significand",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + rounded.get_variable()
                - (F::from(1_u64 << 23), carry.get_variable())
                - output.significand.get_variable()
        },
    );
    Ok(output)
}

fn constrain_constant_fp32<F, CS>(mut cs: CS, value: &AllocatedFp32<F>, expected: CircuitFp32)
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    cs.enforce(
        || "constant exponent",
        |lc| lc,
        |lc| lc,
        |lc| lc + value.exponent.get_variable() - (F::from(expected.exponent as u64), CS::one()),
    );
    cs.enforce(
        || "constant significand",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + value.significand.get_variable()
                - (F::from(expected.significand as u64), CS::one())
        },
    );
    cs.enforce(
        || "constant zero flag",
        |lc| lc,
        |lc| lc,
        |lc| lc + value.is_zero.get_variable() - (F::from(expected.is_zero as u64), CS::one()),
    );
}

/// Decide `x >= 2^(threshold_biased-127)` comparando apenas expoentes.
///
/// O predicado só é exato porque o limiar é uma potência de dois: no perfil
/// positivo-normal restrito, `x >= 2^e` equivale a `expoente(x) >= e+127`. A
/// contagem de constraints não depende do limiar, o que preserva a
/// comparabilidade de custo entre limiares da matriz.
fn decision_at_least_power_of_two<F, CS>(
    mut cs: CS,
    value: &AllocatedFp32<F>,
    witness: CircuitFp32,
    decision_witness: bool,
    threshold_biased: u8,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    assert!(threshold_biased >= 1, "limiar FP32 deve ser normal positivo");
    let below = threshold_biased - 1;
    let decision = allocate_boolean(
        cs.namespace(|| "fp32 threshold decision"),
        "fp32_threshold_decision_value",
        decision_witness,
    )?;
    let true_gap_witness = if decision_witness {
        witness.exponent.saturating_sub(threshold_biased) as u128
    } else {
        0
    };
    let false_gap_witness = if !decision_witness {
        below.saturating_sub(witness.exponent) as u128
    } else {
        0
    };
    let (true_gap, _) = allocate_ranged(
        cs.namespace(|| "fp32 threshold true gap"),
        "fp32_threshold_true_gap_value",
        true_gap_witness,
        8,
    )?;
    let (false_gap, _) = allocate_ranged(
        cs.namespace(|| "fp32 threshold false gap"),
        "fp32_threshold_false_gap_value",
        false_gap_witness,
        8,
    )?;
    cs.enforce(
        || "fp32 true proves exponent at least threshold",
        |lc| lc + decision.get_variable(),
        |lc| {
            lc + value.exponent.get_variable()
                - (F::from(u64::from(threshold_biased)), CS::one())
                - true_gap.get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || "fp32 false proves exponent below threshold",
        |lc| lc + CS::one() - decision.get_variable(),
        |lc| {
            lc + (F::from(u64::from(below)), CS::one())
                - value.exponent.get_variable()
                - false_gap.get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || "fp32 zero cannot decide true",
        |lc| lc + value.is_zero.get_variable(),
        |lc| lc + decision.get_variable(),
        |lc| lc,
    );
    Ok(decision)
}

fn increment<F, CS>(
    mut cs: CS,
    iteration: &AllocatedNum<F>,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let next = AllocatedNum::alloc(cs.namespace(|| "next iteration"), || {
        iteration
            .get_value()
            .map(|value| value + F::ONE)
            .ok_or(SynthesisError::AssignmentMissing)
    })?;
    cs.enforce(
        || "increment iteration",
        |lc| lc,
        |lc| lc,
        |lc| lc + iteration.get_variable() + CS::one() - next.get_variable(),
    );
    Ok(next)
}

impl<F: PrimeField> StepCircuit<F> for Fp32MulCircuit {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert_eq!(z.len(), Self::arity_value());
        let left = constrain_fp32(
            cs.namespace(|| "mul lhs"),
            z[1].clone(),
            z[2].clone(),
            z[3].clone(),
            self.witness.left,
        )?;
        let right = constrain_fp32(
            cs.namespace(|| "mul rhs"),
            z[4].clone(),
            z[5].clone(),
            z[6].clone(),
            self.witness.right,
        )?;
        let output = synthesize_mul(
            cs.namespace(|| "fp32 multiplication"),
            &left,
            &right,
            &self.witness,
        )?;
        let next_iteration = increment(cs.namespace(|| "mul counter"), &z[0])?;
        Ok(vec![
            next_iteration,
            output.exponent,
            output.significand,
            output.is_zero,
            right.exponent,
            right.significand,
            right.is_zero,
        ])
    }
}

impl<F: PrimeField> StepCircuit<F> for Fp32AddCircuit {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert_eq!(z.len(), Self::arity_value());
        let left = constrain_fp32(
            cs.namespace(|| "add lhs"),
            z[1].clone(),
            z[2].clone(),
            z[3].clone(),
            self.witness.left,
        )?;
        let right = constrain_fp32(
            cs.namespace(|| "add rhs"),
            z[4].clone(),
            z[5].clone(),
            z[6].clone(),
            self.witness.right,
        )?;
        let output = synthesize_add(
            cs.namespace(|| "fp32 addition"),
            &left,
            &right,
            &self.witness,
        )?;
        let next_iteration = increment(cs.namespace(|| "add counter"), &z[0])?;
        Ok(vec![
            next_iteration,
            output.exponent,
            output.significand,
            output.is_zero,
            right.exponent,
            right.significand,
            right.is_zero,
        ])
    }
}

impl<F: PrimeField> StepCircuit<F> for Fp32SubOneCircuit {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert_eq!(z.len(), Self::arity_value());
        let input = constrain_fp32(
            cs.namespace(|| "sub input"),
            z[1].clone(),
            z[2].clone(),
            z[3].clone(),
            self.witness.input,
        )?;
        let output = synthesize_sub_one(
            cs.namespace(|| "fp32 subtraction from one"),
            &input,
            &self.witness,
        )?;
        let next_iteration = increment(cs.namespace(|| "sub counter"), &z[0])?;
        Ok(vec![
            next_iteration,
            output.exponent,
            output.significand,
            output.is_zero,
        ])
    }
}

impl<F: PrimeField> StepCircuit<F> for RestrictedFp32StepCircuit {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert_eq!(z.len(), Self::arity_value());
        assert_eq!(self.parameter_bits, self.witness.parameter.bits());
        let input = constrain_fp32(
            cs.namespace(|| "step input"),
            z[1].clone(),
            z[2].clone(),
            z[3].clone(),
            self.witness.input,
        )?;
        let complement = synthesize_sub_one(
            cs.namespace(|| "step one minus x"),
            &input,
            &self.witness.sub_one,
        )?;
        let product = synthesize_mul(
            cs.namespace(|| "step x times complement"),
            &input,
            &complement,
            &self.witness.first_mul,
        )?;
        let parameter = allocate_fp32(
            cs.namespace(|| "step parameter"),
            "step_parameter",
            self.witness.parameter,
        )?;
        constrain_constant_fp32(
            cs.namespace(|| "step fixed parameter"),
            &parameter,
            CircuitFp32::from_bits(self.parameter_bits)
                .map_err(|error| SynthesisError::Unsatisfiable(error.to_string()))?,
        );
        let output = synthesize_mul(
            cs.namespace(|| "step product times parameter"),
            &product,
            &parameter,
            &self.witness.second_mul,
        )?;
        let decision = decision_at_least_power_of_two(
            cs.namespace(|| "step decision"),
            &output,
            self.witness.output,
            self.witness.decision_half,
            self.threshold_biased_exponent,
        )?;
        let next_iteration = increment(cs.namespace(|| "step counter"), &z[0])?;
        Ok(vec![
            next_iteration,
            output.exponent,
            output.significand,
            output.is_zero,
            decision,
        ])
    }
}

impl<F: PrimeField> StepCircuit<F> for RestrictedFp32StableStepCircuit {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert_eq!(z.len(), Self::arity_value());
        let input = constrain_fp32(
            cs.namespace(|| "stable input"),
            z[1].clone(),
            z[2].clone(),
            z[3].clone(),
            self.witness.input,
        )?;
        let alpha = allocate_fp32(
            cs.namespace(|| "stable alpha"),
            "stable_alpha",
            self.witness.alpha,
        )?;
        constrain_constant_fp32(
            cs.namespace(|| "stable fixed alpha"),
            &alpha,
            CircuitFp32::from_bits(self.alpha_bits)
                .map_err(|error| SynthesisError::Unsatisfiable(error.to_string()))?,
        );
        let scaled = synthesize_mul(
            cs.namespace(|| "stable multiplication"),
            &input,
            &alpha,
            &self.witness.multiplication,
        )?;
        let innovation = allocate_fp32(
            cs.namespace(|| "stable innovation"),
            "stable_innovation",
            self.witness.innovation,
        )?;
        constrain_constant_fp32(
            cs.namespace(|| "stable fixed innovation"),
            &innovation,
            CircuitFp32::from_bits(self.innovation_bits)
                .map_err(|error| SynthesisError::Unsatisfiable(error.to_string()))?,
        );
        let output = synthesize_add(
            cs.namespace(|| "stable addition"),
            &scaled,
            &innovation,
            &self.witness.addition,
        )?;
        let decision = decision_at_least_power_of_two(
            cs.namespace(|| "stable decision"),
            &output,
            self.witness.output,
            self.witness.decision_one,
            self.threshold_biased_exponent,
        )?;
        let next_iteration = increment(cs.namespace(|| "stable counter"), &z[0])?;
        Ok(vec![
            next_iteration,
            output.exponent,
            output.significand,
            output.is_zero,
            decision,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_snark::{
        frontend::{test_cs::TestConstraintSystem, ConstraintSystem},
        provider::PallasEngine,
        traits::Engine,
    };

    type Scalar = <PallasEngine as Engine>::Scalar;

    fn alloc_state(
        cs: &mut TestConstraintSystem<Scalar>,
        values: &[u64],
    ) -> Vec<AllocatedNum<Scalar>> {
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                AllocatedNum::alloc(cs.namespace(|| format!("z_{index}")), || {
                    Ok(Scalar::from(*value))
                })
                .unwrap()
            })
            .collect()
    }

    fn synthesize_mul_witness(witness: Fp32MulWitness) -> TestConstraintSystem<Scalar> {
        let mut cs = TestConstraintSystem::new();
        let z = alloc_state(
            &mut cs,
            &[
                0,
                witness.left.exponent as u64,
                witness.left.significand as u64,
                witness.left.is_zero as u64,
                witness.right.exponent as u64,
                witness.right.significand as u64,
                witness.right.is_zero as u64,
            ],
        );
        Fp32MulCircuit { witness }.synthesize(&mut cs, &z).unwrap();
        cs
    }

    fn synthesize_sub_witness(witness: Fp32SubOneWitness) -> TestConstraintSystem<Scalar> {
        let mut cs = TestConstraintSystem::new();
        let z = alloc_state(
            &mut cs,
            &[
                0,
                witness.input.exponent as u64,
                witness.input.significand as u64,
                witness.input.is_zero as u64,
            ],
        );
        Fp32SubOneCircuit { witness }
            .synthesize(&mut cs, &z)
            .unwrap();
        cs
    }

    fn synthesize_step_witness(witness: RestrictedFp32StepWitness) -> TestConstraintSystem<Scalar> {
        let mut cs = TestConstraintSystem::new();
        let z = alloc_state(
            &mut cs,
            &[
                0,
                witness.input.exponent as u64,
                witness.input.significand as u64,
                witness.input.is_zero as u64,
                0,
            ],
        );
        RestrictedFp32StepCircuit {
            witness,
            parameter_bits: 3.7_f32.to_bits(),
            threshold_biased_exponent: HALF_BIASED_EXPONENT,
        }
        .synthesize(&mut cs, &z)
        .unwrap();
        cs
    }

    fn synthesize_add_witness(witness: Fp32AddWitness) -> TestConstraintSystem<Scalar> {
        let mut cs = TestConstraintSystem::new();
        let z = alloc_state(
            &mut cs,
            &[
                0,
                witness.left.exponent as u64,
                witness.left.significand as u64,
                witness.left.is_zero as u64,
                witness.right.exponent as u64,
                witness.right.significand as u64,
                witness.right.is_zero as u64,
            ],
        );
        Fp32AddCircuit { witness }.synthesize(&mut cs, &z).unwrap();
        cs
    }

    fn synthesize_stable_witness(
        witness: RestrictedFp32StableStepWitness,
    ) -> TestConstraintSystem<Scalar> {
        let mut cs = TestConstraintSystem::new();
        let z = alloc_state(
            &mut cs,
            &[
                0,
                witness.input.exponent as u64,
                witness.input.significand as u64,
                witness.input.is_zero as u64,
                0,
            ],
        );
        RestrictedFp32StableStepCircuit {
            witness,
            alpha_bits: 0.5_f32.to_bits(),
            innovation_bits: 0.75_f32.to_bits(),
            threshold_biased_exponent: ONE_BIASED_EXPONENT,
        }
        .synthesize(&mut cs, &z)
        .unwrap();
        cs
    }

    #[test]
    fn multiplication_accepts_normal_and_zero_cases() {
        for (left, right) in [(0.1_f32, 0.9_f32), (0.25, 3.7), (0.0, 3.7)] {
            let witness = Fp32MulWitness::compute(left.to_bits(), right.to_bits()).unwrap();
            let cs = synthesize_mul_witness(witness);
            assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
        }
    }

    #[test]
    fn multiplication_covers_ties_and_rounding_carry() {
        let tie_up = Fp32MulWitness::compute(0x3f80_0001, 0x3fc0_0000).unwrap();
        assert_eq!(
            tie_up.trace.remainder as u64 * 2,
            1_u64 << (23 + usize::from(tie_up.trace.high_product))
        );
        assert!(tie_up.trace.floor & 1 == 1);
        assert!(tie_up.trace.round_up);
        assert!(synthesize_mul_witness(tie_up).is_satisfied());

        let tie_down = Fp32MulWitness::compute(0x3f80_0003, 0x3fc0_0000).unwrap();
        assert_eq!(
            tie_down.trace.remainder as u64 * 2,
            1_u64 << (23 + usize::from(tie_down.trace.high_product))
        );
        assert!(tie_down.trace.floor & 1 == 0);
        assert!(!tie_down.trace.round_up);
        assert!(synthesize_mul_witness(tie_down).is_satisfied());

        let carry = Fp32MulWitness::compute(0x3f80_0001, 0x3fff_fffe).unwrap();
        assert_eq!(carry.trace.rounded, 1 << 24);
        assert_eq!(carry.output.bits(), 2.0_f32.to_bits());
        assert!(synthesize_mul_witness(carry).is_satisfied());
    }

    #[test]
    fn addition_matches_native_fp32_and_has_constant_shape() {
        let mut expected_constraints = None;
        for (left, right) in [
            (0.0_f32, 0.75_f32),
            (0.05, 0.75),
            (0.5, 0.5),
            (0.999_999_94, 0.000_000_059_604_645),
        ] {
            let witness = Fp32AddWitness::compute(left.to_bits(), right.to_bits()).unwrap();
            assert_eq!(witness.output.bits(), (left + right).to_bits());
            let cs = synthesize_add_witness(witness);
            assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
            if let Some(expected) = expected_constraints {
                assert_eq!(cs.num_constraints(), expected);
            } else {
                expected_constraints = Some(cs.num_constraints());
            }
        }
    }

    #[test]
    fn stable_step_matches_reference_and_rejects_tampering() {
        for input in [0.0_f32, 0.1, 0.5, 1.0, 1.5] {
            let witness = RestrictedFp32StableStepWitness::compute(
                input.to_bits(),
                0.5_f32.to_bits(),
                0.75_f32.to_bits(),
            )
            .unwrap();
            assert_eq!(
                witness.output.bits(),
                (0.5_f32 * input + 0.75_f32).to_bits()
            );
            let cs = synthesize_stable_witness(witness);
            assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
        }

        let mut witness = RestrictedFp32StableStepWitness::compute(
            0.5_f32.to_bits(),
            0.5_f32.to_bits(),
            0.75_f32.to_bits(),
        )
        .unwrap();
        witness.decision_one = !witness.decision_one;
        assert!(!synthesize_stable_witness(witness).is_satisfied());
    }

    #[test]
    fn subtraction_accepts_domain_boundaries_and_exponent_alignment() {
        for input in [
            0.0_f32,
            f32::from_bits(1 << 23),
            0.1,
            0.5,
            0.999_999_94,
            1.0,
        ] {
            let witness = Fp32SubOneWitness::compute(input.to_bits()).unwrap();
            let cs = synthesize_sub_witness(witness);
            assert!(
                cs.is_satisfied(),
                "input={input:?}, unsatisfied={:?}",
                cs.which_is_unsatisfied()
            );
        }
    }

    #[test]
    fn complete_step_matches_reference_and_is_satisfied() {
        let mut expected_constraints = None;
        for input in [0.0_f32, 0.1, 0.25, 0.5, 0.9, 1.0] {
            let witness =
                RestrictedFp32StepWitness::compute(input.to_bits(), 3.7_f32.to_bits()).unwrap();
            let cs = synthesize_step_witness(witness);
            assert!(
                cs.is_satisfied(),
                "input={input:?}, unsatisfied={:?}",
                cs.which_is_unsatisfied()
            );
            if let Some(expected) = expected_constraints {
                assert_eq!(
                    cs.num_constraints(),
                    expected,
                    "a forma R1CS não pode depender do witness"
                );
            } else {
                expected_constraints = Some(cs.num_constraints());
            }
        }
    }

    #[test]
    fn rejects_tampered_rounding_and_output() {
        let mut mul = Fp32MulWitness::compute(0.1_f32.to_bits(), 0.9_f32.to_bits()).unwrap();
        mul.trace.round_up = !mul.trace.round_up;
        mul.trace.rounded = mul.trace.floor + u32::from(mul.trace.round_up);
        let cs = synthesize_mul_witness(mul);
        assert!(!cs.is_satisfied());

        let mut step =
            RestrictedFp32StepWitness::compute(0.1_f32.to_bits(), 3.7_f32.to_bits()).unwrap();
        step.output.significand ^= 1;
        step.second_mul.output = step.output;
        let cs = synthesize_step_witness(step);
        assert!(!cs.is_satisfied());
    }

    #[test]
    fn rejects_tampered_decision_and_noncanonical_input() {
        let mut step =
            RestrictedFp32StepWitness::compute(0.1_f32.to_bits(), 3.7_f32.to_bits()).unwrap();
        step.decision_half = !step.decision_half;
        let cs = synthesize_step_witness(step);
        assert!(!cs.is_satisfied());

        let witness =
            RestrictedFp32StepWitness::compute(0.1_f32.to_bits(), 3.7_f32.to_bits()).unwrap();
        let mut cs = TestConstraintSystem::new();
        let z = alloc_state(
            &mut cs,
            &[0, witness.input.exponent as u64, (1 << 23) - 1, 0, 0],
        );
        RestrictedFp32StepCircuit {
            witness,
            parameter_bits: 3.7_f32.to_bits(),
            threshold_biased_exponent: HALF_BIASED_EXPONENT,
        }
        .synthesize(&mut cs, &z)
        .unwrap();
        assert!(!cs.is_satisfied());
    }

    #[test]
    fn ten_thousand_random_steps_match_native_fp32() {
        let mut state = 0x05ee_d754_u32;
        for index in 0..10_000 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let exponent = 1 + ((state >> 24) % 126);
            let fraction = state & 0x7f_ffff;
            let input_bits = (exponent << 23) | fraction;
            let witness =
                RestrictedFp32StepWitness::compute(input_bits, 3.7_f32.to_bits()).unwrap();
            let reference = restricted_logistic_step(input_bits, 3.7_f32.to_bits()).unwrap();
            assert_eq!(witness.output.bits(), reference.output.bits());
            if index < 256 {
                let cs = synthesize_step_witness(witness);
                assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
            }
        }
    }
}

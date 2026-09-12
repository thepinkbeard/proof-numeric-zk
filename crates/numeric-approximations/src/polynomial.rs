use crate::{
    conceptual_value_bytes, pade_diagonal_coefficients, polynomial_exact, taylor_coefficients,
    ApproximationError, OperationCounts,
};
use num_rational::BigRational;
use num_traits::{Signed, Zero};
use numeric_core::{FixedPointFormat, FunctionId, SignedFixedPoint};

#[derive(Clone, Debug)]
pub struct PolynomialEvaluation {
    pub input_quantized: BigRational,
    pub input_quantization_error: BigRational,
    pub ideal_value: BigRational,
    pub quantized_coefficients_ideal_value: BigRational,
    pub fixed_value: BigRational,
    pub coefficient_quantization_error_sum: BigRational,
    pub operation_quantization_error: BigRational,
    pub numerator_coefficients: Vec<BigRational>,
    pub denominator_coefficients: Option<Vec<BigRational>>,
    pub counts: OperationCounts,
}

pub fn evaluate_taylor(
    function: FunctionId,
    degree: usize,
    input: &BigRational,
    format: &FixedPointFormat,
) -> Result<PolynomialEvaluation, ApproximationError> {
    let coefficients = taylor_coefficients(function, degree)?;
    let input_encoded = format.encode(input)?;
    let input_quantized = input_encoded.value.decode();
    let ideal_value = polynomial_exact(&coefficients, &input_quantized);
    let (fixed, quantized_coefficients, coefficient_error, mut counts) =
        evaluate_polynomial_fixed(&coefficients, &input_encoded.value, format)?;
    let quantized_ideal = polynomial_exact(&quantized_coefficients, &input_quantized);
    counts.verification_relation_depth = Some(2 * floor_log2(degree.saturating_sub(1)) as u32);
    Ok(PolynomialEvaluation {
        input_quantized,
        input_quantization_error: input_encoded.exact_error,
        ideal_value,
        quantized_coefficients_ideal_value: quantized_ideal.clone(),
        fixed_value: fixed.decode(),
        coefficient_quantization_error_sum: coefficient_error,
        operation_quantization_error: fixed.decode() - quantized_ideal,
        numerator_coefficients: coefficients,
        denominator_coefficients: None,
        counts,
    })
}

pub fn evaluate_pade(
    function: FunctionId,
    order: usize,
    input: &BigRational,
    format: &FixedPointFormat,
) -> Result<PolynomialEvaluation, ApproximationError> {
    let (numerator, denominator) = pade_diagonal_coefficients(function, order)?;
    let input_encoded = format.encode(input)?;
    let input_quantized = input_encoded.value.decode();
    let ideal_denominator = polynomial_exact(&denominator, &input_quantized);
    if ideal_denominator.is_zero() {
        return Err(ApproximationError::ZeroPadeDenominator);
    }
    let ideal_value = polynomial_exact(&numerator, &input_quantized) / ideal_denominator;
    let (fixed_numerator, quantized_numerator, numerator_error, mut counts) =
        evaluate_polynomial_fixed(&numerator, &input_encoded.value, format)?;
    let (fixed_denominator, quantized_denominator, denominator_error, denominator_counts) =
        evaluate_polynomial_fixed(&denominator, &input_encoded.value, format)?;
    counts.add_assign(&denominator_counts);
    let divided = fixed_numerator.div(&fixed_denominator)?;
    counts.divisions += 1;
    counts.inversions += 1;
    counts.numerical_multiplicative_depth = order as u32 + 1;
    counts.verification_relation_depth = Some(3);
    counts.conceptual_witness_bytes += conceptual_value_bytes(format);
    let quantized_ideal_denominator = polynomial_exact(&quantized_denominator, &input_quantized);
    if quantized_ideal_denominator.is_zero() {
        return Err(ApproximationError::ZeroPadeDenominator);
    }
    let quantized_ideal =
        polynomial_exact(&quantized_numerator, &input_quantized) / quantized_ideal_denominator;
    Ok(PolynomialEvaluation {
        input_quantized,
        input_quantization_error: input_encoded.exact_error,
        ideal_value,
        quantized_coefficients_ideal_value: quantized_ideal.clone(),
        fixed_value: divided.value.decode(),
        coefficient_quantization_error_sum: numerator_error.abs() + denominator_error.abs(),
        operation_quantization_error: divided.value.decode() - quantized_ideal,
        numerator_coefficients: numerator,
        denominator_coefficients: Some(denominator),
        counts,
    })
}

fn evaluate_polynomial_fixed(
    coefficients: &[BigRational],
    input: &SignedFixedPoint,
    format: &FixedPointFormat,
) -> Result<
    (
        SignedFixedPoint,
        Vec<BigRational>,
        BigRational,
        OperationCounts,
    ),
    ApproximationError,
> {
    let encoded: Vec<_> = coefficients
        .iter()
        .map(|coefficient| format.encode(coefficient))
        .collect::<Result<_, _>>()?;
    let quantized_coefficients: Vec<_> = encoded.iter().map(|value| value.value.decode()).collect();
    let coefficient_error = encoded
        .iter()
        .fold(BigRational::zero(), |accumulator, value| {
            accumulator + value.exact_error.abs()
        });
    let mut accumulator = encoded
        .last()
        .expect("aproximação sempre tem coeficiente")
        .value
        .clone();
    for coefficient in encoded.iter().rev().skip(1) {
        accumulator = accumulator.mul(input)?.value;
        accumulator = accumulator.add(&coefficient.value)?;
    }
    let degree = coefficients.len().saturating_sub(1);
    let counts = OperationCounts {
        additions: degree as u64,
        multiplications: degree as u64,
        terms: coefficients.len() as u64,
        numerical_multiplicative_depth: degree as u32,
        conceptual_witness_bytes: conceptual_value_bytes(format)
            * (2 * coefficients.len() as u64 + 1),
        ..OperationCounts::default()
    };
    Ok((
        accumulator,
        quantized_coefficients,
        coefficient_error,
        counts,
    ))
}

fn floor_log2(value: usize) -> usize {
    if value <= 1 {
        0
    } else {
        usize::BITS as usize - value.leading_zeros() as usize - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use numeric_core::{OverflowMode, QuantizationRounding};

    fn format() -> FixedPointFormat {
        FixedPointFormat {
            integer_bits: 16,
            fractional_bits: 32,
            signed: true,
            rounding: QuantizationRounding::NearestTiesToEven,
            overflow: OverflowMode::Reject,
        }
    }

    #[test]
    fn taylor_and_pade_are_exact_at_exp_zero() {
        let zero = BigRational::zero();
        let taylor = evaluate_taylor(FunctionId::Exp, 9, &zero, &format()).unwrap();
        let pade = evaluate_pade(FunctionId::Exp, 5, &zero, &format()).unwrap();
        assert_eq!(taylor.fixed_value, BigRational::from_integer(1.into()));
        assert_eq!(pade.fixed_value, BigRational::from_integer(1.into()));
    }

    #[test]
    fn sigmoid_is_half_at_zero_for_both_methods() {
        let zero = BigRational::zero();
        assert_eq!(
            evaluate_taylor(FunctionId::Sigmoid, 9, &zero, &format())
                .unwrap()
                .fixed_value,
            BigRational::new(1.into(), 2.into())
        );
        assert_eq!(
            evaluate_pade(FunctionId::Sigmoid, 5, &zero, &format())
                .unwrap()
                .fixed_value,
            BigRational::new(1.into(), 2.into())
        );
    }
}

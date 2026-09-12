use crate::{conceptual_value_bytes, ApproximationError, OperationCounts};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use numeric_core::{FixedPointFormat, FunctionId, SignedFixedPoint};
use numeric_reference::{float_to_big_rational, gauss_legendre_nodes_weights};

#[derive(Clone, Debug)]
pub struct GaussLegendreEvaluation {
    pub input_quantized: BigRational,
    pub input_quantization_error: BigRational,
    pub ideal_value: BigRational,
    pub fixed_value: BigRational,
    pub relation_residual: BigRational,
    /// Paper-derived four-bit aggregate slack, expressed in relation space.
    pub paper_relation_slack_bound: BigRational,
    /// Analytical conversion from relation-space slack to output-space around
    /// the quadrature solution. It does not include quadrature approximation
    /// error relative to the transcendental function.
    pub verification_output_bound: BigRational,
    pub solver_iterations: u32,
    pub counts: OperationCounts,
}

#[derive(Clone, Debug)]
pub struct PreparedGaussLegendre {
    pub function: FunctionId,
    pub order: usize,
    pub nodes: Vec<BigRational>,
    pub weights: Vec<BigRational>,
}

pub fn prepare_gauss_legendre(
    function: FunctionId,
    order: usize,
) -> Result<PreparedGaussLegendre, ApproximationError> {
    if !matches!(function, FunctionId::Exp | FunctionId::Sigmoid) {
        return Err(ApproximationError::UnsupportedFunction);
    }
    let rule = gauss_legendre_nodes_weights(order, 512)
        .map_err(|error| ApproximationError::InvalidGaussLegendreRule(error.to_string()))?;
    Ok(PreparedGaussLegendre {
        function,
        order,
        nodes: rule.nodes.iter().map(float_to_big_rational).collect(),
        weights: rule.weights.iter().map(float_to_big_rational).collect(),
    })
}

pub fn evaluate_gauss_legendre(
    function: FunctionId,
    order: usize,
    input: &BigRational,
    format: &FixedPointFormat,
) -> Result<GaussLegendreEvaluation, ApproximationError> {
    let prepared = prepare_gauss_legendre(function, order)?;
    evaluate_gauss_legendre_prepared(&prepared, input, format)
}

pub fn evaluate_gauss_legendre_prepared(
    prepared: &PreparedGaussLegendre,
    input: &BigRational,
    format: &FixedPointFormat,
) -> Result<GaussLegendreEvaluation, ApproximationError> {
    let function = prepared.function;
    let nodes = &prepared.nodes;
    let weights = &prepared.weights;
    let input_encoded = format.encode(input)?;
    let input_quantized = input_encoded.value.decode();
    let (ideal_value, solver_iterations) = solve_implicit_relation(
        function,
        &input_quantized,
        nodes,
        weights,
        usize::from(format.fractional_bits).max(32) + 32,
    )?;
    // The honest result is the canonical quantization of the implicit
    // quadrature solution. Searching neighboring grid points for a smaller
    // residual would model an adversarial prover and belongs to the separate
    // verification-slack analysis.
    let selected = format.encode(&ideal_value)?.value;
    let (residual, mut counts) = fixed_relation(
        function,
        &input_encoded.value,
        &selected,
        nodes,
        weights,
        format,
    )?;
    counts.verification_relation_depth = Some(4);
    let relation_slack = BigRational::new(BigInt::from(16), format.scale());
    let derivative_inverse_bound = match function {
        // For y in [1/4,4] and r in [-1,1], each stable denominator is
        // at most 8; sum(w)=2 gives g'(y)>=1/16. Use 16 conservatively.
        FunctionId::Exp => BigRational::from_integer(16.into()),
        // For y in [1/4,3/4], denominator <=3/2 and g'(y)>=16/9.
        FunctionId::Sigmoid => BigRational::new(9.into(), 16.into()),
        _ => unreachable!(),
    };
    let verification_output_bound = &relation_slack * derivative_inverse_bound;
    Ok(GaussLegendreEvaluation {
        input_quantized,
        input_quantization_error: input_encoded.exact_error,
        ideal_value,
        fixed_value: selected.decode(),
        relation_residual: residual,
        paper_relation_slack_bound: relation_slack,
        verification_output_bound,
        solver_iterations,
        counts,
    })
}

pub fn gauss_legendre_relation(
    function: FunctionId,
    input: &BigRational,
    output: &BigRational,
    nodes: &[BigRational],
    weights: &[BigRational],
) -> Result<BigRational, ApproximationError> {
    if nodes.len() != weights.len() || nodes.is_empty() {
        return Err(ApproximationError::InvalidGaussLegendreRule(
            "nodes e weights devem ter o mesmo tamanho positivo".into(),
        ));
    }
    let one = BigRational::one();
    let two = BigRational::from_integer(2.into());
    let mut sum = BigRational::zero();
    for (node, weight) in nodes.iter().zip(weights) {
        let (numerator, denominator) = match function {
            FunctionId::Exp => {
                let numerator = output - &one;
                let denominator = output * node - node + output + &one;
                (numerator, denominator)
            }
            FunctionId::Sigmoid => {
                let numerator = &two * output - &one;
                let denominator = &two * output * node - node + &one;
                (numerator, denominator)
            }
            _ => return Err(ApproximationError::UnsupportedFunction),
        };
        if denominator.is_zero() {
            return Err(ApproximationError::UnbracketedGaussLegendreRoot);
        }
        sum += weight * numerator / denominator;
    }
    Ok(sum - input)
}

fn solve_implicit_relation(
    function: FunctionId,
    input: &BigRational,
    nodes: &[BigRational],
    weights: &[BigRational],
    iterations: usize,
) -> Result<(BigRational, u32), ApproximationError> {
    let (mut lower, mut upper) = match function {
        FunctionId::Exp => (
            BigRational::new(1.into(), 4.into()),
            BigRational::from_integer(4.into()),
        ),
        FunctionId::Sigmoid => (
            BigRational::new(1.into(), 4.into()),
            BigRational::new(3.into(), 4.into()),
        ),
        _ => return Err(ApproximationError::UnsupportedFunction),
    };
    let lower_residual = gauss_legendre_relation(function, input, &lower, nodes, weights)?;
    let upper_residual = gauss_legendre_relation(function, input, &upper, nodes, weights)?;
    if lower_residual > BigRational::zero() || upper_residual < BigRational::zero() {
        return Err(ApproximationError::UnbracketedGaussLegendreRoot);
    }
    for _ in 0..iterations {
        let midpoint = (&lower + &upper) / BigRational::from_integer(2.into());
        let residual = gauss_legendre_relation(function, input, &midpoint, nodes, weights)?;
        if residual.is_zero() {
            return Ok((midpoint, iterations as u32));
        }
        if residual.is_negative() {
            lower = midpoint;
        } else {
            upper = midpoint;
        }
    }
    Ok((
        (&lower + &upper) / BigRational::from_integer(2.into()),
        iterations as u32,
    ))
}

fn fixed_relation(
    function: FunctionId,
    input: &SignedFixedPoint,
    output: &SignedFixedPoint,
    nodes: &[BigRational],
    weights: &[BigRational],
    format: &FixedPointFormat,
) -> Result<(BigRational, OperationCounts), ApproximationError> {
    let one = format.encode(&BigRational::one())?.value;
    let quantized_nodes: Vec<_> = nodes
        .iter()
        .map(|value| format.encode(value).map(|encoded| encoded.value))
        .collect::<Result<_, _>>()?;
    let quantized_weights: Vec<_> = weights
        .iter()
        .map(|value| format.encode(value).map(|encoded| encoded.value))
        .collect::<Result<_, _>>()?;
    let mut sum = format.encode(&BigRational::zero())?.value;
    for (node, weight) in quantized_nodes.iter().zip(&quantized_weights) {
        let (numerator, denominator) = match function {
            FunctionId::Exp => {
                let numerator = output.sub(&one)?;
                let yr = output.mul(node)?.value;
                let denominator = yr.sub(node)?.add(output)?.add(&one)?;
                (numerator, denominator)
            }
            FunctionId::Sigmoid => {
                let twice_output = output.add(output)?;
                let numerator = twice_output.sub(&one)?;
                let yr = output.mul(node)?.value;
                let twice_yr = yr.add(&yr)?;
                let denominator = twice_yr.sub(node)?.add(&one)?;
                (numerator, denominator)
            }
            _ => return Err(ApproximationError::UnsupportedFunction),
        };
        let inverse_term = numerator.div(&denominator)?.value;
        let weighted = weight.mul(&inverse_term)?.value;
        sum = sum.add(&weighted)?;
    }
    let residual = sum.sub(input)?.decode();
    let per_term_additions = match function {
        FunctionId::Exp => 5,
        FunctionId::Sigmoid => 6,
        _ => unreachable!(),
    };
    let count = nodes.len() as u64;
    Ok((
        residual,
        OperationCounts {
            additions: per_term_additions * count + 1,
            multiplications: 2 * count,
            divisions: count,
            inversions: count,
            terms: count,
            numerical_multiplicative_depth: 3,
            conceptual_witness_bytes: conceptual_value_bytes(format) * (5 * count + 3),
            ..OperationCounts::default()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use numeric_core::{OverflowMode, QuantizationRounding};

    fn format(bits: u16) -> FixedPointFormat {
        FixedPointFormat {
            integer_bits: bits,
            fractional_bits: bits,
            signed: true,
            rounding: QuantizationRounding::NearestTiesToEven,
            overflow: OverflowMode::Reject,
        }
    }

    #[test]
    fn gl_relations_recover_exact_values_at_zero() {
        for function in [FunctionId::Exp, FunctionId::Sigmoid] {
            let evaluation =
                evaluate_gauss_legendre(function, 8, &BigRational::zero(), &format(32)).unwrap();
            let expected = if function == FunctionId::Exp {
                BigRational::one()
            } else {
                BigRational::new(1.into(), 2.into())
            };
            assert_eq!(evaluation.fixed_value, expected);
            assert!(evaluation.relation_residual.abs() <= evaluation.paper_relation_slack_bound);
        }
    }

    #[test]
    fn gl_width_grows_but_relation_depth_stays_constant() {
        let low = evaluate_gauss_legendre(
            FunctionId::Sigmoid,
            2,
            &BigRational::new(1.into(), 3.into()),
            &format(32),
        )
        .unwrap();
        let high = evaluate_gauss_legendre(
            FunctionId::Sigmoid,
            16,
            &BigRational::new(1.into(), 3.into()),
            &format(32),
        )
        .unwrap();
        assert!(high.counts.multiplications > low.counts.multiplications);
        assert_eq!(low.counts.verification_relation_depth, Some(4));
        assert_eq!(high.counts.verification_relation_depth, Some(4));
    }

    #[test]
    fn q16_order_four_matches_official_artifact_constants() {
        let prepared = prepare_gauss_legendre(FunctionId::Sigmoid, 4).unwrap();
        let format = format(16);
        let roots: Vec<BigInt> = prepared
            .nodes
            .iter()
            .map(|value| format.encode(value).unwrap().value.raw().clone())
            .collect();
        let weights: Vec<BigInt> = prepared
            .weights
            .iter()
            .map(|value| format.encode(value).unwrap().value.raw().clone())
            .collect();
        assert_eq!(
            roots,
            vec![(-56435).into(), (-22281).into(), 22281.into(), 56435.into()]
        );
        assert_eq!(
            weights,
            vec![22797.into(), 42739.into(), 42739.into(), 22797.into()]
        );
    }
}

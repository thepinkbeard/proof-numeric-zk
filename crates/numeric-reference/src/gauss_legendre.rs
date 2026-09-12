//! Deterministic MPFR generation of Gauss-Legendre nodes and weights.

use rug::{float::Constant, ops::Pow, Assign, Float};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct GaussLegendreRule {
    pub order: usize,
    pub precision_bits: u32,
    /// Nodes in strictly ascending order.
    pub nodes: Vec<Float>,
    /// Positive weights aligned with `nodes`.
    pub weights: Vec<Float>,
}

pub fn gauss_legendre_nodes_weights(
    order: usize,
    precision_bits: u32,
) -> Result<GaussLegendreRule, GaussLegendreError> {
    if order == 0 || order > 128 {
        return Err(GaussLegendreError::InvalidOrder);
    }
    let precision = precision_bits.max(128);
    let half = order.div_ceil(2);
    let tolerance = Float::with_val(precision, 2).pow(-((precision - 24) as i32));
    let pi = Float::with_val(precision, Constant::Pi);
    let mut positive = Vec::with_capacity(half);

    for index in 0..half {
        // Standard asymptotic start for the positive roots, followed by an
        // MPFR Newton solve of P_n(x)=0.
        let numerator = Float::with_val(precision, index as f64 + 0.75);
        let denominator = Float::with_val(precision, order as f64 + 0.5);
        let mut angle = Float::with_val(precision, &pi * numerator);
        angle /= denominator;
        let mut root = angle.cos();
        let mut derivative = Float::with_val(precision, 0);
        let mut converged = false;
        for _ in 0..128 {
            let (pn, pnm1) = legendre_pair(order, &root, precision);
            let mut numerator = Float::with_val(precision, &root * &pn);
            numerator -= pnm1;
            numerator *= order;
            let mut denominator = Float::with_val(precision, &root * &root);
            denominator -= 1;
            derivative.assign(&numerator / denominator);
            let correction = Float::with_val(precision, &pn / &derivative);
            let next = Float::with_val(precision, &root - &correction);
            let difference = Float::with_val(precision, &next - &root).abs();
            root = next;
            if difference <= tolerance {
                converged = true;
                break;
            }
        }
        if !converged {
            return Err(GaussLegendreError::NoConvergence { index });
        }
        let (pn, pnm1) = legendre_pair(order, &root, precision);
        let mut numerator = Float::with_val(precision, &root * pn);
        numerator -= pnm1;
        numerator *= order;
        let mut denominator = Float::with_val(precision, &root * &root);
        denominator -= 1;
        derivative.assign(&numerator / denominator);
        let mut weight_denominator = Float::with_val(precision, 1);
        weight_denominator -= Float::with_val(precision, &root * &root);
        weight_denominator *= Float::with_val(precision, &derivative * &derivative);
        let weight = Float::with_val(precision, 2) / weight_denominator;
        positive.push((root, weight));
    }

    let mut pairs = Vec::with_capacity(order);
    for (root, weight) in &positive {
        if root.is_zero() {
            pairs.push((root.clone(), weight.clone()));
        } else {
            pairs.push((-root.clone(), weight.clone()));
            pairs.push((root.clone(), weight.clone()));
        }
    }
    pairs.sort_by(|left, right| left.0.partial_cmp(&right.0).unwrap());
    if pairs.len() != order {
        return Err(GaussLegendreError::InvalidRootCount {
            expected: order,
            actual: pairs.len(),
        });
    }
    let (nodes, weights): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    Ok(GaussLegendreRule {
        order,
        precision_bits: precision,
        nodes,
        weights,
    })
}

fn legendre_pair(order: usize, x: &Float, precision: u32) -> (Float, Float) {
    if order == 0 {
        return (Float::with_val(precision, 1), Float::with_val(precision, 0));
    }
    let mut pnm1 = Float::with_val(precision, 1);
    let mut pn = Float::with_val(precision, x);
    if order == 1 {
        return (pn, pnm1);
    }
    for degree in 2..=order {
        let mut next = Float::with_val(precision, x * &pn);
        next *= 2 * degree - 1;
        let mut previous_term = Float::with_val(precision, &pnm1);
        previous_term *= degree - 1;
        next -= previous_term;
        next /= degree;
        pnm1 = pn;
        pn = next;
    }
    (pn, pnm1)
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum GaussLegendreError {
    #[error("ordem Gauss-Legendre deve estar em 1..=128")]
    InvalidOrder,
    #[error("Newton não convergiu para a raiz de índice {index}")]
    NoConvergence { index: usize },
    #[error("quantidade de raízes inválida: esperado {expected}, obtido {actual}")]
    InvalidRootCount { expected: usize, actual: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_two_matches_closed_form_and_weights_sum_two() {
        let rule = gauss_legendre_nodes_weights(2, 256).unwrap();
        let expected = Float::with_val(256, 1) / Float::with_val(256, 3).sqrt();
        assert!(
            Float::with_val(256, &rule.nodes[0] + &expected).abs() < Float::with_val(256, 1e-60)
        );
        assert!(
            Float::with_val(256, &rule.nodes[1] - &expected).abs() < Float::with_val(256, 1e-60)
        );
        let sum = rule
            .weights
            .iter()
            .fold(Float::with_val(256, 0), |mut acc, weight| {
                acc += weight;
                acc
            });
        assert!(Float::with_val(256, sum - 2).abs() < Float::with_val(256, 1e-60));
    }

    #[test]
    fn generated_rules_are_symmetric_positive_and_ordered() {
        for order in [2, 4, 8, 16, 32] {
            let rule = gauss_legendre_nodes_weights(order, 256).unwrap();
            assert!(rule.nodes.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(rule.weights.iter().all(|weight| weight > &0));
            for index in 0..order {
                let mirror = order - index - 1;
                assert!(
                    Float::with_val(256, &rule.nodes[index] + &rule.nodes[mirror]).abs()
                        < Float::with_val(256, 1e-60)
                );
                assert!(
                    Float::with_val(256, &rule.weights[index] - &rule.weights[mirror]).abs()
                        < Float::with_val(256, 1e-60)
                );
            }
        }
    }
}

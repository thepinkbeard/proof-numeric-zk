//! Referência matemática independente baseada em racionais exatos.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Zero};
use rug::{
    float::Round,
    ops::{MulAssignRound, Pow, SubAssignRound},
    Float, Integer, Rational,
};
use thiserror::Error;

mod fp32;
mod dynamic_interval;
pub use dynamic_interval::directed_dynamic_trajectory;
mod gauss_legendre;
mod transcendental;

pub use fp32::{
    restricted_logistic_step, restricted_stable_step, RestrictedFp32, RestrictedFp32Error,
    RestrictedFp32StableTrace, RestrictedFp32Trace,
};
pub use gauss_legendre::{gauss_legendre_nodes_weights, GaussLegendreError, GaussLegendreRule};
pub use transcendental::{
    evaluate_transcendental, float_to_big_rational, resolve_transcendental_decision, ReferenceEnclosure,
    TranscendentalReferenceError,
};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ReferenceError {
    #[error("x0 deve pertencer a [0,1]")]
    InvalidInitialValue,
    #[error("r deve pertencer a [0,4]")]
    InvalidParameter,
}

pub fn decimal_ratio(numerator: i64, denominator: i64) -> BigRational {
    BigRational::new(BigInt::from(numerator), BigInt::from(denominator))
}

pub fn logistic_step(x: &BigRational, r: &BigRational) -> BigRational {
    r * x * (BigRational::one() - x)
}

pub fn logistic_trajectory(
    x0: &BigRational,
    r: &BigRational,
    steps: usize,
) -> Result<Vec<BigRational>, ReferenceError> {
    if x0 < &BigRational::zero() || x0 > &BigRational::one() {
        return Err(ReferenceError::InvalidInitialValue);
    }
    if r < &BigRational::zero() || r > &BigRational::from_integer(4.into()) {
        return Err(ReferenceError::InvalidParameter);
    }
    let mut trajectory = Vec::with_capacity(steps.saturating_add(1));
    trajectory.push(x0.clone());
    for _ in 0..steps {
        let next = logistic_step(trajectory.last().expect("trajetória não vazia"), r);
        trajectory.push(next);
    }
    Ok(trajectory)
}

#[derive(Clone, Debug)]
pub struct HighPrecisionConfig {
    pub precision_bits: u32,
}

impl Default for HighPrecisionConfig {
    fn default() -> Self {
        Self {
            precision_bits: 256,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HighPrecisionPoint {
    pub value: Float,
    /// Bound conservador acumulado calculado por propagação a posteriori.
    pub estimated_absolute_error: Float,
}

pub fn rational_to_float(value: &BigRational, precision: u32) -> Float {
    let numerator =
        Integer::from_str_radix(&value.numer().to_string(), 10).expect("BigInt decimal válido");
    let denominator =
        Integer::from_str_radix(&value.denom().to_string(), 10).expect("BigInt decimal válido");
    Float::with_val_round(
        precision,
        Rational::from((numerator, denominator)),
        Round::Nearest,
    )
    .0
}

/// Trajetória MPFR com arredondamento explícito para o mais próximo.
///
/// O bound usa `|f'(x)| <= r <= 4`: `e_(n+1) <= 4 e_n + 2^(4-p)`.
/// A parcela local é deliberadamente conservadora e cobre conversão de `r` e
/// três operações MPFR para valores intermediários limitados por quatro.
pub fn logistic_trajectory_high_precision(
    x0: &BigRational,
    r: &BigRational,
    steps: usize,
    config: &HighPrecisionConfig,
) -> Result<Vec<HighPrecisionPoint>, ReferenceError> {
    if x0 < &BigRational::zero() || x0 > &BigRational::one() {
        return Err(ReferenceError::InvalidInitialValue);
    }
    if r < &BigRational::zero() || r > &BigRational::from_integer(4.into()) {
        return Err(ReferenceError::InvalidParameter);
    }
    let precision = config.precision_bits.max(32);
    let r_float = rational_to_float(r, precision);
    let mut x = rational_to_float(x0, precision);
    let mut error = Float::with_val(precision, 2).pow(-(precision as i32));
    let round_bound = Float::with_val(precision, 2).pow(4 - precision as i32);
    let mut trajectory = Vec::with_capacity(steps.saturating_add(1));
    trajectory.push(HighPrecisionPoint {
        value: x.clone(),
        estimated_absolute_error: error.clone(),
    });
    for _ in 0..steps {
        let mut one_minus = Float::with_val(precision, 1);
        one_minus.sub_assign_round(&x, Round::Nearest);
        let mut next = Float::with_val(precision, &r_float);
        next.mul_assign_round(&x, Round::Nearest);
        next.mul_assign_round(&one_minus, Round::Nearest);
        error *= 4;
        error += &round_bound;
        x = next;
        trajectory.push(HighPrecisionPoint {
            value: x.clone(),
            estimated_absolute_error: error.clone(),
        });
    }
    Ok(trajectory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_step_is_exact() {
        let x0 = decimal_ratio(1, 10);
        let r = decimal_ratio(7, 2);
        let trajectory = logistic_trajectory(&x0, &r, 1).unwrap();
        assert_eq!(trajectory[1], decimal_ratio(63, 200));
    }

    #[test]
    fn rejects_invalid_domains() {
        assert_eq!(
            logistic_trajectory(&decimal_ratio(11, 10), &decimal_ratio(4, 1), 1),
            Err(ReferenceError::InvalidInitialValue)
        );
    }

    #[test]
    fn high_precision_matches_exact_first_step() {
        let x0 = decimal_ratio(1, 10);
        let r = decimal_ratio(7, 2);
        let hp = logistic_trajectory_high_precision(
            &x0,
            &r,
            1,
            &HighPrecisionConfig {
                precision_bits: 256,
            },
        )
        .unwrap();
        let exact = Float::with_val(256, Rational::from((63, 200)));
        let mut difference = hp[1].value.clone();
        difference -= exact;
        assert!(difference.abs() < Float::with_val(256, 1e-70));
    }
}

//! Directed-MPFR references for primitive non-polynomial functions.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use numeric_core::FunctionId;
use rug::{
    float::Round,
    ops::{AddAssignRound, NegAssign},
    Float, Integer, Rational,
};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct ReferenceEnclosure {
    pub precision_bits: u32,
    pub lower: Float,
    pub nearest: Float,
    pub upper: Float,
    pub binary32: f32,
    pub auxiliary_f64: f64,
}

impl ReferenceEnclosure {
    /// Decide only when the entire directed enclosure determines `y >= tau`.
    pub fn ge_threshold(&self, threshold: &BigRational) -> Option<bool> {
        if self.lower_rational() >= *threshold {
            Some(true)
        } else if self.upper_rational() < *threshold {
            Some(false)
        } else {
            None
        }
    }

    pub fn lower_rational(&self) -> BigRational {
        float_to_big_rational(&self.lower)
    }

    pub fn nearest_rational(&self) -> BigRational {
        float_to_big_rational(&self.nearest)
    }

    pub fn upper_rational(&self) -> BigRational {
        float_to_big_rational(&self.upper)
    }

    pub fn contains(&self, value: &BigRational) -> bool {
        self.lower_rational() <= *value && *value <= self.upper_rational()
    }
}

/// Refine an inconclusive reference up to a public precision limit.
/// `None` is not a false decision and must not be counted as agreement.
pub fn resolve_transcendental_decision(
    function: FunctionId,
    input: &BigRational,
    threshold: &BigRational,
    initial_bits: u32,
    maximum_bits: u32,
) -> Result<(Option<bool>, u32), TranscendentalReferenceError> {
    if initial_bits < 64 || maximum_bits < initial_bits || maximum_bits > 16384 {
        return Err(TranscendentalReferenceError::InvalidPrecision);
    }
    let mut bits = initial_bits;
    loop {
        let value = evaluate_transcendental(function, input, bits)?;
        let decision = value.ge_threshold(threshold);
        if decision.is_some() || bits == maximum_bits {
            return Ok((decision, bits));
        }
        bits = (bits * 2).min(maximum_bits);
    }
}

pub fn evaluate_transcendental(
    function: FunctionId,
    input: &BigRational,
    precision_bits: u32,
) -> Result<ReferenceEnclosure, TranscendentalReferenceError> {
    let precision = precision_bits.max(64);
    let lower_input = rational_to_float_round(input, precision, Round::Down);
    let nearest_input = rational_to_float_round(input, precision, Round::Nearest);
    let upper_input = rational_to_float_round(input, precision, Round::Up);

    let (lower, nearest, upper) = match function {
        FunctionId::Exp => (
            exp_round(lower_input, Round::Down),
            exp_round(nearest_input, Round::Nearest),
            exp_round(upper_input, Round::Up),
        ),
        FunctionId::Sigmoid => (
            sigmoid_lower(lower_input),
            sigmoid_nearest(nearest_input),
            sigmoid_upper(upper_input),
        ),
        unsupported => return Err(TranscendentalReferenceError::Unsupported(unsupported)),
    };
    if !lower.is_finite() || !nearest.is_finite() || !upper.is_finite() {
        return Err(TranscendentalReferenceError::NonFinite);
    }
    if lower > nearest || nearest > upper {
        return Err(TranscendentalReferenceError::InvalidEnclosure);
    }
    Ok(ReferenceEnclosure {
        precision_bits: precision,
        binary32: nearest.to_f32_round(Round::Nearest),
        auxiliary_f64: nearest.to_f64_round(Round::Nearest),
        lower,
        nearest,
        upper,
    })
}

fn rational_to_float_round(value: &BigRational, precision: u32, round: Round) -> Float {
    let numerator =
        Integer::from_str_radix(&value.numer().to_string(), 10).expect("BigInt decimal válido");
    let denominator =
        Integer::from_str_radix(&value.denom().to_string(), 10).expect("BigInt decimal válido");
    Float::with_val_round(precision, Rational::from((numerator, denominator)), round).0
}

fn exp_round(mut value: Float, round: Round) -> Float {
    value.exp_round(round);
    value
}

fn sigmoid_lower(lower_x: Float) -> Float {
    // sigma is monotone. For a lower result, compute exp(-x) upward,
    // add upward, then take the reciprocal downward.
    let mut denominator = lower_x;
    denominator.neg_assign();
    denominator.exp_round(Round::Up);
    denominator.add_assign_round(1, Round::Up);
    denominator.recip_round(Round::Down);
    denominator
}

fn sigmoid_upper(upper_x: Float) -> Float {
    let mut denominator = upper_x;
    denominator.neg_assign();
    denominator.exp_round(Round::Down);
    denominator.add_assign_round(1, Round::Down);
    denominator.recip_round(Round::Up);
    denominator
}

fn sigmoid_nearest(nearest_x: Float) -> Float {
    let mut denominator = nearest_x;
    denominator.neg_assign();
    denominator.exp_round(Round::Nearest);
    denominator.add_assign_round(1, Round::Nearest);
    denominator.recip_round(Round::Nearest);
    denominator
}

pub fn float_to_big_rational(value: &Float) -> BigRational {
    let (mantissa, exponent) = value
        .to_integer_exp()
        .expect("referência MPFR deve ser finita");
    let hexadecimal = mantissa.to_string_radix(16);
    let negative = hexadecimal.starts_with('-');
    let magnitude = BigInt::parse_bytes(hexadecimal.trim_start_matches('-').as_bytes(), 16)
        .expect("mantissa hexadecimal válida");
    let signed = if negative { -magnitude } else { magnitude };
    if exponent >= 0 {
        BigRational::from_integer(signed << exponent as usize)
    } else {
        BigRational::new(signed, BigInt::one() << (-exponent) as usize)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum TranscendentalReferenceError {
    #[error("precisão dirigida deve satisfazer 64 <= initial <= maximum <= 16384")]
    InvalidPrecision,
    #[error("função ainda não suportada pela referência: {0:?}")]
    Unsupported(FunctionId),
    #[error("referência MPFR produziu valor não finito")]
    NonFinite,
    #[error("arredondamento dirigido produziu enclosure inválido")]
    InvalidEnclosure,
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_traits::Zero;

    #[test]
    fn boundary_requires_refinement_not_nearest_value() {
        let x = BigRational::new(1.into(), 3.into());
        let low = evaluate_transcendental(FunctionId::Exp, &x, 64).unwrap();
        let threshold = (low.lower_rational() + low.upper_rational()) / BigInt::from(2);
        assert_eq!(low.ge_threshold(&threshold), None);
        let (decision, bits) =
            resolve_transcendental_decision(FunctionId::Exp, &x, &threshold, 64, 512).unwrap();
        assert!(decision.is_some());
        assert!(bits > 64);
        assert_eq!(
            resolve_transcendental_decision(FunctionId::Exp, &x, &threshold, 64, 64,)
                .unwrap()
                .0,
            None
        );
        assert_eq!(
            resolve_transcendental_decision(
                FunctionId::Exp,
                &BigRational::zero(),
                &BigRational::one(),
                64,
                512,
            )
            .unwrap()
            .0,
            Some(true)
        );
    }

    #[test]
    fn exp_zero_encloses_one() {
        let reference =
            evaluate_transcendental(FunctionId::Exp, &BigRational::zero(), 256).unwrap();
        assert!(reference.contains(&BigRational::one()));
        assert_eq!(reference.binary32, 1.0);
    }

    #[test]
    fn sigmoid_zero_encloses_half() {
        let reference =
            evaluate_transcendental(FunctionId::Sigmoid, &BigRational::zero(), 256).unwrap();
        assert!(reference.contains(&BigRational::new(1.into(), 2.into())));
        assert_eq!(reference.binary32, 0.5);
    }

    #[test]
    fn higher_precision_nearest_is_inside_lower_precision_enclosure() {
        let x = BigRational::new(7.into(), 13.into());
        for function in [FunctionId::Exp, FunctionId::Sigmoid] {
            let low = evaluate_transcendental(function, &x, 256).unwrap();
            let high = evaluate_transcendental(function, &x, 512).unwrap();
            assert!(low.contains(&high.nearest_rational()));
        }
    }
}

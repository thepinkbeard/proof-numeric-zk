//! Exact signed fixed-point simulation for semantic experiments.

use crate::{
    NumericContractProfile, NumericRepresentationKind, OverflowMode, QuantizationRounding,
};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FixedPointFormat {
    pub integer_bits: u16,
    pub fractional_bits: u16,
    pub signed: bool,
    pub rounding: QuantizationRounding,
    pub overflow: OverflowMode,
}

impl FixedPointFormat {
    pub fn from_contract(profile: &NumericContractProfile) -> Result<Self, FixedPointError> {
        if profile.representation.kind != NumericRepresentationKind::SignedFixedPoint {
            return Err(FixedPointError::WrongRepresentation);
        }
        Ok(Self {
            integer_bits: profile.representation.integer_bits,
            fractional_bits: profile.representation.fractional_bits,
            signed: profile.representation.signed,
            rounding: profile.operations.rounding,
            overflow: profile.operations.overflow,
        })
    }

    pub fn validate(&self) -> Result<(), FixedPointError> {
        if self.integer_bits == 0 || self.fractional_bits == 0 || !self.signed {
            return Err(FixedPointError::InvalidFormat);
        }
        Ok(())
    }

    pub fn scale(&self) -> BigInt {
        BigInt::one() << self.fractional_bits
    }

    pub fn minimum_raw(&self) -> BigInt {
        -(BigInt::one() << (self.integer_bits + self.fractional_bits))
    }

    pub fn maximum_raw(&self) -> BigInt {
        (BigInt::one() << (self.integer_bits + self.fractional_bits)) - BigInt::one()
    }

    pub fn ulp(&self) -> BigRational {
        BigRational::new(BigInt::one(), self.scale())
    }

    pub fn absolute_quantization_bound(&self) -> BigRational {
        match self.rounding {
            QuantizationRounding::NearestTiesToEven => self.ulp() / BigInt::from(2),
            QuantizationRounding::Floor | QuantizationRounding::TowardZero => self.ulp(),
        }
    }

    pub fn encode(&self, input: &BigRational) -> Result<FixedPointQuantization, FixedPointError> {
        self.validate()?;
        let scaled = input * self.scale();
        let raw = round_rational_to_integer(&scaled, self.rounding);
        let value = SignedFixedPoint::from_raw(self.clone(), raw)?;
        let exact_error = value.decode() - input;
        Ok(FixedPointQuantization { value, exact_error })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SignedFixedPoint {
    format: FixedPointFormat,
    raw: BigInt,
}

impl SignedFixedPoint {
    pub fn from_raw(format: FixedPointFormat, raw: BigInt) -> Result<Self, FixedPointError> {
        format.validate()?;
        if raw < format.minimum_raw() || raw > format.maximum_raw() {
            return Err(FixedPointError::Overflow);
        }
        Ok(Self { format, raw })
    }

    pub fn raw(&self) -> &BigInt {
        &self.raw
    }

    pub fn format(&self) -> &FixedPointFormat {
        &self.format
    }

    pub fn decode(&self) -> BigRational {
        BigRational::new(self.raw.clone(), self.format.scale())
    }

    pub fn add(&self, rhs: &Self) -> Result<Self, FixedPointError> {
        self.ensure_same_format(rhs)?;
        Self::from_raw(self.format.clone(), &self.raw + &rhs.raw)
    }

    pub fn sub(&self, rhs: &Self) -> Result<Self, FixedPointError> {
        self.ensure_same_format(rhs)?;
        Self::from_raw(self.format.clone(), &self.raw - &rhs.raw)
    }

    pub fn mul(&self, rhs: &Self) -> Result<FixedPointQuantization, FixedPointError> {
        self.ensure_same_format(rhs)?;
        self.format.encode(&(self.decode() * rhs.decode()))
    }

    pub fn div(&self, rhs: &Self) -> Result<FixedPointQuantization, FixedPointError> {
        self.ensure_same_format(rhs)?;
        if rhs.raw.is_zero() {
            return Err(FixedPointError::DivisionByZero);
        }
        self.format.encode(&(self.decode() / rhs.decode()))
    }

    fn ensure_same_format(&self, rhs: &Self) -> Result<(), FixedPointError> {
        if self.format != rhs.format {
            return Err(FixedPointError::FormatMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedPointQuantization {
    pub value: SignedFixedPoint,
    /// `quantized - exact`, represented as an exact rational.
    pub exact_error: BigRational,
}

pub fn round_rational_to_integer(value: &BigRational, rounding: QuantizationRounding) -> BigInt {
    let numerator = value.numer();
    let denominator = value.denom();
    match rounding {
        QuantizationRounding::TowardZero => numerator / denominator,
        QuantizationRounding::Floor => {
            let quotient = numerator / denominator;
            let remainder = numerator % denominator;
            if numerator.is_negative() && !remainder.is_zero() {
                quotient - BigInt::one()
            } else {
                quotient
            }
        }
        QuantizationRounding::NearestTiesToEven => {
            let sign_is_negative = numerator.is_negative();
            let magnitude = numerator.abs();
            let quotient = &magnitude / denominator;
            let remainder = &magnitude % denominator;
            let doubled = &remainder << 1_usize;
            let increment = doubled > *denominator
                || (doubled == *denominator && (&quotient & BigInt::one()) == BigInt::one());
            let rounded = quotient
                + if increment {
                    BigInt::one()
                } else {
                    BigInt::zero()
                };
            if sign_is_negative {
                -rounded
            } else {
                rounded
            }
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FixedPointError {
    #[error("formato fixed-point inválido")]
    InvalidFormat,
    #[error("perfil não usa signed fixed-point")]
    WrongRepresentation,
    #[error("overflow fixed-point rejeitado")]
    Overflow,
    #[error("operação entre formatos fixed-point diferentes")]
    FormatMismatch,
    #[error("divisão fixed-point por zero")]
    DivisionByZero,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(rounding: QuantizationRounding) -> FixedPointFormat {
        FixedPointFormat {
            integer_bits: 2,
            fractional_bits: 2,
            signed: true,
            rounding,
            overflow: OverflowMode::Reject,
        }
    }

    #[test]
    fn floor_and_truncation_differ_for_negative_values() {
        let input = BigRational::new((-5).into(), 8.into());
        assert_eq!(
            format(QuantizationRounding::Floor)
                .encode(&input)
                .unwrap()
                .value
                .raw(),
            &BigInt::from(-3)
        );
        assert_eq!(
            format(QuantizationRounding::TowardZero)
                .encode(&input)
                .unwrap()
                .value
                .raw(),
            &BigInt::from(-2)
        );
    }

    #[test]
    fn nearest_rounds_half_to_even_symmetrically() {
        let f = format(QuantizationRounding::NearestTiesToEven);
        assert_eq!(
            f.encode(&BigRational::new(5.into(), 8.into()))
                .unwrap()
                .value
                .raw(),
            &BigInt::from(2)
        );
        assert_eq!(
            f.encode(&BigRational::new(7.into(), 8.into()))
                .unwrap()
                .value
                .raw(),
            &BigInt::from(4)
        );
        assert_eq!(
            f.encode(&BigRational::new((-5).into(), 8.into()))
                .unwrap()
                .value
                .raw(),
            &BigInt::from(-2)
        );
        assert_eq!(
            f.encode(&BigRational::new((-7).into(), 8.into()))
                .unwrap()
                .value
                .raw(),
            &BigInt::from(-4)
        );
    }

    #[test]
    fn encode_decode_and_arithmetic_are_exactly_auditable() {
        let f = format(QuantizationRounding::NearestTiesToEven);
        let a = f
            .encode(&BigRational::new(3.into(), 2.into()))
            .unwrap()
            .value;
        let b = f
            .encode(&BigRational::new(1.into(), 2.into()))
            .unwrap()
            .value;
        assert_eq!(
            a.add(&b).unwrap().decode(),
            BigRational::from_integer(2.into())
        );
        assert_eq!(
            a.mul(&b).unwrap().value.decode(),
            BigRational::new(3.into(), 4.into())
        );
    }

    #[test]
    fn rejects_overflow() {
        let f = FixedPointFormat {
            integer_bits: 1,
            fractional_bits: 2,
            signed: true,
            rounding: QuantizationRounding::NearestTiesToEven,
            overflow: OverflowMode::Reject,
        };
        assert_eq!(
            f.encode(&BigRational::from_integer(2.into())).unwrap_err(),
            FixedPointError::Overflow
        );
    }
}

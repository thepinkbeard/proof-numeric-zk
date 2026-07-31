//! Aritmética fracionária em limbs com perda de precisão explícita.

mod cbd;
mod cfd;
mod frontier;
mod profile;

use num_bigint::{BigInt, BigUint, Sign};
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use cbd::{CbdError, CertifiedDyadic, Decision3, NumericProfile, OverflowPolicy};
pub use cfd::{CertifiedFloat, CfdError, CfdOperation, CfdProfile, CfdRoundingPolicy};
pub use frontier::{FrontierPoint, RepresentationId, WorkloadSpec};
pub use profile::{
    BoundMode, ExperimentProfile, ExperimentProfileBuilder, ProfileError, ProfileFamily,
    RoundingId, Topology, WorkloadId, PROFILE_SCHEMA_VERSION,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RoundingMode {
    Truncate,
    NearestTiesToEven,
    NearestTiesAway,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuantizationResult<T> {
    pub value: T,
    /// `value - input`, calculado exatamente.
    pub exact_error: BigRational,
    pub rounded_up: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MulResult<const P: usize, const K: usize> {
    pub value: LimbNumber<P, K>,
    /// Resto não negativo `d` na igualdade `Nx * Ny = S*q + d`.
    pub discarded: BigUint,
    pub denominator_power: usize,
    pub rounded_up: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScaledMulResult<const P: usize, const K: usize> {
    pub value: LimbNumber<P, K>,
    /// Resto `d` em `numerator * factor_num = factor_den * q + d`.
    pub discarded: BigUint,
    pub divisor: BigUint,
    pub rounded_up: bool,
}

/// Estado em uma grade fina `S²`, decomposto como `S*V + E`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompensatedNumber<const P: usize, const K: usize> {
    value: LimbNumber<P, K>,
    residual: BigInt,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum NumericError {
    #[error("P deve estar no intervalo 1..=63")]
    InvalidLimbWidth,
    #[error("K deve ser positivo")]
    InvalidLimbCount,
    #[error("valor fora do domínio [0,1]")]
    OutOfDomain,
    #[error("limb {index}={value} fora do intervalo [0, 2^P)")]
    LimbOutOfRange { index: usize, value: u64 },
    #[error("representação não canônica do valor um")]
    NonCanonicalOne,
    #[error("resultado fora do domínio [0,1]")]
    Overflow,
    #[error("subtração produziria valor negativo")]
    Underflow,
    #[error("f64 deve ser finito e pertencer a [0,1]")]
    InvalidFloat,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LimbNumber<const P: usize, const K: usize> {
    limbs: [u64; K],
    is_one: bool,
}

impl<const P: usize, const K: usize> LimbNumber<P, K> {
    pub fn new(limbs: [u64; K], is_one: bool) -> Result<Self, NumericError> {
        Self::validate_parameters()?;
        let bound = 1_u64 << P;
        for (index, &value) in limbs.iter().enumerate() {
            if value >= bound {
                return Err(NumericError::LimbOutOfRange { index, value });
            }
        }
        if is_one && limbs.iter().any(|limb| *limb != 0) {
            return Err(NumericError::NonCanonicalOne);
        }
        Ok(Self { limbs, is_one })
    }

    pub fn zero() -> Self {
        assert!(P > 0 && P <= 63 && K > 0);
        Self {
            limbs: [0; K],
            is_one: false,
        }
    }

    pub fn one() -> Self {
        assert!(P > 0 && P <= 63 && K > 0);
        Self {
            limbs: [0; K],
            is_one: true,
        }
    }

    pub fn limbs(&self) -> &[u64; K] {
        &self.limbs
    }

    pub fn is_one(&self) -> bool {
        self.is_one
    }

    pub fn fractional_bits() -> usize {
        P * K
    }

    pub fn scale() -> BigUint {
        BigUint::one() << Self::fractional_bits()
    }

    pub fn numerator(&self) -> BigUint {
        if self.is_one {
            return Self::scale();
        }
        let mut result = BigUint::zero();
        for limb in self.limbs {
            result = (result << P) + limb;
        }
        result
    }

    pub fn to_big_rational(&self) -> BigRational {
        BigRational::new(
            BigInt::from_biguint(Sign::Plus, self.numerator()),
            BigInt::from_biguint(Sign::Plus, Self::scale()),
        )
    }

    pub fn to_f64_for_display(&self) -> f64 {
        self.to_big_rational().to_f64().unwrap_or(f64::NAN)
    }

    pub fn from_f64(
        value: f64,
        mode: RoundingMode,
    ) -> Result<QuantizationResult<Self>, NumericError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(NumericError::InvalidFloat);
        }
        let rational = BigRational::from_float(value).ok_or(NumericError::InvalidFloat)?;
        Self::from_rational(&rational, mode)
    }

    pub fn from_rational(
        input: &BigRational,
        mode: RoundingMode,
    ) -> Result<QuantizationResult<Self>, NumericError> {
        Self::validate_parameters()?;
        if input.is_negative() || input > &BigRational::one() {
            return Err(NumericError::OutOfDomain);
        }

        let scale = BigInt::from_biguint(Sign::Plus, Self::scale());
        let scaled_num = input.numer() * &scale;
        let denominator = input.denom();
        let quotient = &scaled_num / denominator;
        let remainder = &scaled_num % denominator;
        let rounded_up = should_round_up(&quotient, &remainder, denominator, mode);
        let selected = quotient
            + if rounded_up {
                BigInt::one()
            } else {
                BigInt::zero()
            };
        let selected_u = selected.to_biguint().ok_or(NumericError::OutOfDomain)?;
        let value = Self::from_scaled_numerator(&selected_u)?;
        let exact_error = value.to_big_rational() - input;
        Ok(QuantizationResult {
            value,
            exact_error,
            rounded_up,
        })
    }

    pub fn add(&self, rhs: &Self) -> Result<Self, NumericError> {
        let sum = self.numerator() + rhs.numerator();
        Self::from_scaled_numerator(&sum)
    }

    pub fn sub(&self, rhs: &Self) -> Result<Self, NumericError> {
        let left = self.numerator();
        let right = rhs.numerator();
        if left < right {
            return Err(NumericError::Underflow);
        }
        Self::from_scaled_numerator(&(left - right))
    }

    pub fn one_minus(&self) -> Self {
        Self::from_scaled_numerator(&(Self::scale() - self.numerator()))
            .expect("x validado em [0,1]")
    }

    pub fn mul(&self, rhs: &Self, mode: RoundingMode) -> Result<MulResult<P, K>, NumericError> {
        let raw = self.convolution_product(rhs);
        debug_assert_eq!(raw, self.numerator() * rhs.numerator());
        let scale = Self::scale();
        let quotient = &raw / &scale;
        let discarded = &raw % &scale;
        let rounded_up_big = should_round_up_biguint(&quotient, &discarded, &scale, mode);
        let selected = quotient
            + if rounded_up_big {
                BigUint::one()
            } else {
                BigUint::zero()
            };
        let value = Self::from_scaled_numerator(&selected)?;
        Ok(MulResult {
            value,
            discarded,
            denominator_power: 2,
            rounded_up: rounded_up_big,
        })
    }

    /// Multiplica por um racional público não negativo, retornando a divisão
    /// inteira e o resto que seriam constrained no circuito.
    pub fn mul_ratio(
        &self,
        factor_num: &BigUint,
        factor_den: &BigUint,
        mode: RoundingMode,
    ) -> Result<ScaledMulResult<P, K>, NumericError> {
        if factor_den.is_zero() {
            return Err(NumericError::Overflow);
        }
        let raw = self.numerator() * factor_num;
        let divisor = factor_den.clone();
        let quotient = &raw / &divisor;
        let discarded = &raw % &divisor;
        let rounded_up = should_round_up_biguint(&quotient, &discarded, &divisor, mode);
        let selected = quotient
            + if rounded_up {
                BigUint::one()
            } else {
                BigUint::zero()
            };
        Ok(ScaledMulResult {
            value: Self::from_scaled_numerator(&selected)?,
            discarded,
            divisor,
            rounded_up,
        })
    }

    /// Produto exato obtido por convolução posicional, antes do reescalonamento.
    pub fn convolution_product(&self, rhs: &Self) -> BigUint {
        if self.is_one || rhs.is_one {
            return self.numerator() * rhs.numerator();
        }
        let mut coefficients = vec![BigUint::zero(); K.saturating_mul(2)];
        for (i, &a) in self.limbs.iter().rev().enumerate() {
            for (j, &b) in rhs.limbs.iter().rev().enumerate() {
                coefficients[i + j] += BigUint::from(a) * BigUint::from(b);
            }
        }
        let base = BigUint::one() << P;
        let mut carry = BigUint::zero();
        for coefficient in &mut coefficients {
            *coefficient += &carry;
            carry = &*coefficient / &base;
            *coefficient %= &base;
        }
        while !carry.is_zero() {
            coefficients.push(&carry % &base);
            carry /= &base;
        }
        coefficients
            .into_iter()
            .rev()
            .fold(BigUint::zero(), |acc, digit| acc * &base + digit)
    }

    fn from_scaled_numerator(numerator: &BigUint) -> Result<Self, NumericError> {
        let scale = Self::scale();
        if numerator > &scale {
            return Err(NumericError::Overflow);
        }
        if numerator == &scale {
            return Ok(Self::one());
        }
        let mask = (BigUint::one() << P) - BigUint::one();
        let mut remaining = numerator.clone();
        let mut limbs = [0_u64; K];
        for limb in limbs.iter_mut().rev() {
            *limb = (&remaining & &mask)
                .to_u64()
                .expect("um limb com P <= 63 cabe em u64");
            remaining >>= P;
        }
        debug_assert!(remaining.is_zero());
        Self::new(limbs, false)
    }

    fn validate_parameters() -> Result<(), NumericError> {
        if P == 0 || P > 63 {
            return Err(NumericError::InvalidLimbWidth);
        }
        if K == 0 {
            return Err(NumericError::InvalidLimbCount);
        }
        Ok(())
    }
}

impl<const P: usize, const K: usize> CompensatedNumber<P, K> {
    pub fn from_rational(
        input: &BigRational,
        mode: RoundingMode,
    ) -> Result<QuantizationResult<Self>, NumericError> {
        if input.is_negative() || input > &BigRational::one() {
            return Err(NumericError::OutOfDomain);
        }
        let scale = LimbNumber::<P, K>::scale();
        let fine_scale = &scale * &scale;
        let fine_scale_i = BigInt::from_biguint(Sign::Plus, fine_scale.clone());
        let scaled_num = input.numer() * &fine_scale_i;
        let quotient = &scaled_num / input.denom();
        let remainder = &scaled_num % input.denom();
        let rounded_up = should_round_up(&quotient, &remainder, input.denom(), mode);
        let fine_numerator = quotient
            + if rounded_up {
                BigInt::one()
            } else {
                BigInt::zero()
            };
        let value = Self::from_fine_numerator(&fine_numerator)?;
        let exact_error = value.to_big_rational() - input;
        Ok(QuantizationResult {
            value,
            exact_error,
            rounded_up,
        })
    }

    pub fn value(&self) -> &LimbNumber<P, K> {
        &self.value
    }

    pub fn residual(&self) -> &BigInt {
        &self.residual
    }

    pub fn fine_numerator(&self) -> BigInt {
        let scale = BigInt::from_biguint(Sign::Plus, LimbNumber::<P, K>::scale());
        BigInt::from_biguint(Sign::Plus, self.value.numerator()) * scale + &self.residual
    }

    pub fn to_big_rational(&self) -> BigRational {
        let scale = LimbNumber::<P, K>::scale();
        let fine_scale = &scale * &scale;
        BigRational::new(
            self.fine_numerator(),
            BigInt::from_biguint(Sign::Plus, fine_scale),
        )
    }

    pub fn logistic_step(
        &self,
        r: &BigRational,
        mode: RoundingMode,
    ) -> Result<QuantizationResult<Self>, NumericError> {
        let x = self.to_big_rational();
        let exact_next = r * &x * (BigRational::one() - x);
        Self::from_rational(&exact_next, mode)
    }

    fn from_fine_numerator(fine: &BigInt) -> Result<Self, NumericError> {
        let scale_u = LimbNumber::<P, K>::scale();
        let fine_scale = &scale_u * &scale_u;
        let fine_u = fine.to_biguint().ok_or(NumericError::OutOfDomain)?;
        if fine_u > fine_scale {
            return Err(NumericError::Overflow);
        }
        let scale = BigInt::from_biguint(Sign::Plus, scale_u.clone());
        // Centra E no intervalo aproximadamente [-S/2,S/2].
        let half = &scale >> 1_usize;
        let mut main = (fine + &half) / &scale;
        let max_main = scale.clone();
        if main > max_main {
            main = max_main;
        }
        let residual = fine - &main * &scale;
        let main_u = main.to_biguint().ok_or(NumericError::OutOfDomain)?;
        let value = LimbNumber::<P, K>::from_scaled_numerator(&main_u)?;
        Ok(Self { value, residual })
    }
}

fn should_round_up(
    quotient: &BigInt,
    remainder: &BigInt,
    denominator: &BigInt,
    mode: RoundingMode,
) -> bool {
    if remainder.is_zero() || mode == RoundingMode::Truncate {
        return false;
    }
    let doubled: BigInt = remainder << 1_usize;
    match doubled.cmp(denominator) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match mode {
            RoundingMode::Truncate => false,
            RoundingMode::NearestTiesAway => true,
            RoundingMode::NearestTiesToEven => (quotient & BigInt::one()) == BigInt::one(),
        },
    }
}

fn should_round_up_biguint(
    quotient: &BigUint,
    remainder: &BigUint,
    denominator: &BigUint,
    mode: RoundingMode,
) -> bool {
    if remainder.is_zero() || mode == RoundingMode::Truncate {
        return false;
    }
    let doubled: BigUint = remainder << 1_usize;
    match doubled.cmp(denominator) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match mode {
            RoundingMode::Truncate => false,
            RoundingMode::NearestTiesAway => true,
            RoundingMode::NearestTiesToEven => (quotient & BigUint::one()) == BigUint::one(),
        },
    }
}

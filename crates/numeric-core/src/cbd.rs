//! Referência racional para a representação Certified Bounded Dyadic (CBD).

use crate::RoundingMode;
use num_bigint::{BigInt, BigUint, Sign};
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum OverflowPolicy {
    Reject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Decision3 {
    False,
    True,
    Indeterminate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NumericProfile {
    id: String,
    version: u16,
    fractional_bits: usize,
    mantissa_magnitude_bits: usize,
    minimum: BigRational,
    maximum: BigRational,
    rounding: RoundingMode,
    overflow: OverflowPolicy,
    thresholds: Vec<BigRational>,
    next_fractional_bits: Option<usize>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CbdError {
    #[error("perfil CBD inválido: {0}")]
    InvalidProfile(&'static str),
    #[error("intervalo CBD inválido")]
    InvalidInterval,
    #[error("valor ou intervalo fora do domínio público")]
    OutsideDomain,
    #[error("operação entre perfis diferentes")]
    ProfileMismatch,
    #[error("o raio informado não é o raio canônico mínimo")]
    NonCanonicalRadius,
    #[error("o perfil não define uma precisão seguinte")]
    NoNextProfile,
    #[error("parâmetro público não suportado pela operação")]
    UnsupportedParameter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertifiedDyadic {
    profile: NumericProfile,
    mantissa: BigInt,
    radius: BigUint,
}

impl NumericProfile {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        version: u16,
        fractional_bits: usize,
        mantissa_magnitude_bits: usize,
        minimum: BigRational,
        maximum: BigRational,
        thresholds: Vec<BigRational>,
        next_fractional_bits: Option<usize>,
    ) -> Result<Self, CbdError> {
        if fractional_bits == 0 {
            return Err(CbdError::InvalidProfile(
                "fractional_bits deve ser positivo",
            ));
        }
        if mantissa_magnitude_bits == 0 {
            return Err(CbdError::InvalidProfile(
                "mantissa_magnitude_bits deve ser positivo",
            ));
        }
        if minimum >= maximum {
            return Err(CbdError::InvalidProfile(
                "minimum deve ser estritamente menor que maximum",
            ));
        }
        if thresholds
            .iter()
            .any(|threshold| threshold < &minimum || threshold > &maximum)
        {
            return Err(CbdError::InvalidProfile(
                "todo limiar deve pertencer ao domínio",
            ));
        }
        if next_fractional_bits.is_some_and(|next| next <= fractional_bits) {
            return Err(CbdError::InvalidProfile(
                "a precisão seguinte deve ser estritamente maior",
            ));
        }

        let scale = BigInt::one() << fractional_bits;
        let magnitude_limit = BigInt::one() << mantissa_magnitude_bits;
        let minimum_scaled = floor_rational(&(minimum.clone() * &scale));
        let maximum_scaled = ceil_rational(&(maximum.clone() * &scale));
        if minimum_scaled <= -&magnitude_limit || maximum_scaled >= magnitude_limit {
            return Err(CbdError::InvalidProfile(
                "domínio não cabe na largura de magnitude declarada",
            ));
        }

        Ok(Self {
            id: id.into(),
            version,
            fractional_bits,
            mantissa_magnitude_bits,
            minimum,
            maximum,
            rounding: RoundingMode::NearestTiesToEven,
            overflow: OverflowPolicy::Reject,
            thresholds,
            next_fractional_bits,
        })
    }

    pub fn bounded_unit(fractional_bits: usize) -> Result<Self, CbdError> {
        let next_fractional_bits = match fractional_bits {
            16 => Some(32),
            32 => Some(48),
            48 => Some(64),
            _ => None,
        };
        Self::new(
            format!("cbd-unit-{fractional_bits}"),
            1,
            fractional_bits,
            fractional_bits + 1,
            BigRational::zero(),
            BigRational::one(),
            vec![
                BigRational::new(1.into(), 4.into()),
                BigRational::new(1.into(), 2.into()),
                BigRational::new(3.into(), 4.into()),
            ],
            next_fractional_bits,
        )
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn fractional_bits(&self) -> usize {
        self.fractional_bits
    }

    pub fn mantissa_magnitude_bits(&self) -> usize {
        self.mantissa_magnitude_bits
    }

    pub fn minimum(&self) -> &BigRational {
        &self.minimum
    }

    pub fn maximum(&self) -> &BigRational {
        &self.maximum
    }

    pub fn rounding(&self) -> RoundingMode {
        self.rounding
    }

    pub fn overflow(&self) -> OverflowPolicy {
        self.overflow
    }

    pub fn thresholds(&self) -> &[BigRational] {
        &self.thresholds
    }

    pub fn next_fractional_bits(&self) -> Option<usize> {
        self.next_fractional_bits
    }

    pub fn scale(&self) -> BigInt {
        BigInt::one() << self.fractional_bits
    }

    pub fn next_profile(&self) -> Result<Self, CbdError> {
        let bits = self.next_fractional_bits.ok_or(CbdError::NoNextProfile)?;
        Self::new(
            format!("{}-escalated-{bits}", self.id),
            self.version,
            bits,
            bits + 1,
            self.minimum.clone(),
            self.maximum.clone(),
            self.thresholds.clone(),
            match bits {
                16 => Some(32),
                32 => Some(48),
                48 => Some(64),
                _ => None,
            },
        )
    }
}

impl CertifiedDyadic {
    pub fn from_exact(profile: NumericProfile, exact: &BigRational) -> Result<Self, CbdError> {
        Self::from_interval_with_center(profile, exact, exact, exact)
    }

    pub fn from_parts(
        profile: NumericProfile,
        mantissa: BigInt,
        radius: BigUint,
    ) -> Result<Self, CbdError> {
        let value = Self {
            profile,
            mantissa,
            radius,
        };
        value.validate_domain()?;
        Ok(value)
    }

    pub fn profile(&self) -> &NumericProfile {
        &self.profile
    }

    pub fn mantissa(&self) -> &BigInt {
        &self.mantissa
    }

    pub fn radius(&self) -> &BigUint {
        &self.radius
    }

    pub fn center(&self) -> BigRational {
        BigRational::new(self.mantissa.clone(), self.profile.scale())
    }

    pub fn lower(&self) -> BigRational {
        let radius = BigInt::from_biguint(Sign::Plus, self.radius.clone());
        BigRational::new(&self.mantissa - radius, self.profile.scale())
            .max(self.profile.minimum.clone())
    }

    pub fn upper(&self) -> BigRational {
        let radius = BigInt::from_biguint(Sign::Plus, self.radius.clone());
        BigRational::new(&self.mantissa + radius, self.profile.scale())
            .min(self.profile.maximum.clone())
    }

    pub fn contains(&self, exact: &BigRational) -> bool {
        self.lower() <= *exact && *exact <= self.upper()
    }

    pub fn is_canonical_for_interval(&self, lower: &BigRational, upper: &BigRational) -> bool {
        if lower > upper {
            return false;
        }
        minimal_radius(&self.center(), lower, upper, &self.profile)
            .is_ok_and(|radius| radius == self.radius)
    }

    pub fn add(&self, rhs: &Self) -> Result<Self, CbdError> {
        self.ensure_same_profile(rhs)?;
        let center = self.center() + rhs.center();
        let lower = self.lower() + rhs.lower();
        let upper = self.upper() + rhs.upper();
        Self::from_interval_with_center(self.profile.clone(), &center, &lower, &upper)
    }

    pub fn sub(&self, rhs: &Self) -> Result<Self, CbdError> {
        self.ensure_same_profile(rhs)?;
        let center = self.center() - rhs.center();
        let lower = self.lower() - rhs.upper();
        let upper = self.upper() - rhs.lower();
        Self::from_interval_with_center(self.profile.clone(), &center, &lower, &upper)
    }

    pub fn mul(&self, rhs: &Self) -> Result<Self, CbdError> {
        self.ensure_same_profile(rhs)?;
        let center = self.center() * rhs.center();
        let products = [
            self.lower() * rhs.lower(),
            self.lower() * rhs.upper(),
            self.upper() * rhs.lower(),
            self.upper() * rhs.upper(),
        ];
        let lower = products.iter().min().expect("quatro produtos").clone();
        let upper = products.iter().max().expect("quatro produtos").clone();
        Self::from_interval_with_center(self.profile.clone(), &center, &lower, &upper)
    }

    pub fn mul_public(&self, factor: &BigRational) -> Result<Self, CbdError> {
        let center = self.center() * factor;
        let endpoint_a = self.lower() * factor;
        let endpoint_b = self.upper() * factor;
        let lower = endpoint_a.clone().min(endpoint_b.clone());
        let upper = endpoint_a.max(endpoint_b);
        Self::from_interval_with_center(self.profile.clone(), &center, &lower, &upper)
    }

    /// Calcula `self * multiplier + addend` com uma única quantização do
    /// centro. O intervalo usa os extremos exatos dos operandos.
    pub fn fma(&self, multiplier: &Self, addend: &Self) -> Result<Self, CbdError> {
        self.ensure_same_profile(multiplier)?;
        self.ensure_same_profile(addend)?;
        let center = self.center() * multiplier.center() + addend.center();
        let products = [
            self.lower() * multiplier.lower(),
            self.lower() * multiplier.upper(),
            self.upper() * multiplier.lower(),
            self.upper() * multiplier.upper(),
        ];
        let product_lower = products.iter().min().expect("quatro produtos").clone();
        let product_upper = products.iter().max().expect("quatro produtos").clone();
        let lower = product_lower + addend.lower();
        let upper = product_upper + addend.upper();
        Self::from_interval_with_center(self.profile.clone(), &center, &lower, &upper)
    }

    /// Avalia `r*x*(1-x)` e quantiza o centro somente ao final da expressão.
    ///
    /// O enclosure usa a monotonicidade da parábola em cada lado de `1/2`.
    /// A versão inicial exige `r >= 0` e um perfil contido em `[0,1]`.
    pub fn logistic_step_fused(&self, r: &BigRational) -> Result<Self, CbdError> {
        if r.is_negative()
            || self.profile.minimum < BigRational::zero()
            || self.profile.maximum > BigRational::one()
        {
            return Err(CbdError::UnsupportedParameter);
        }
        let logistic = |x: &BigRational| r * x * (BigRational::one() - x);
        let lower_input = self.lower();
        let upper_input = self.upper();
        let center = logistic(&self.center());
        let lower_endpoint = logistic(&lower_input);
        let upper_endpoint = logistic(&upper_input);
        let lower = lower_endpoint.clone().min(upper_endpoint.clone());
        let mut upper = lower_endpoint.max(upper_endpoint);
        let half = BigRational::new(1.into(), 2.into());
        if lower_input <= half && half <= upper_input {
            upper = upper.max(logistic(&half));
        }
        Self::from_interval_with_center(self.profile.clone(), &center, &lower, &upper)
    }

    /// Ablation com quantização após `x*(1-x)` e após multiplicar por `r`.
    pub fn logistic_step_sequential(&self, r: &BigRational) -> Result<Self, CbdError> {
        let one = Self::from_exact(self.profile.clone(), &BigRational::one())?;
        let complement = one.sub(self)?;
        self.mul(&complement)?.mul_public(r)
    }

    /// Semântica determinística usada pela primeira step circuit CBD.
    ///
    /// Em `[0,1]`, `|d(x(1-x))/dx| <= 1`; portanto o erro de entrada em ULPs
    /// cresce no máximo por `r`. Uma ULP adicional cobre a quantização do
    /// centro quando ela é inexata. O raio satura no domínio completo.
    pub fn logistic_step_lipschitz(&self, r: &BigRational) -> Result<Self, CbdError> {
        if r.is_negative()
            || self.profile.minimum < BigRational::zero()
            || self.profile.maximum > BigRational::one()
        {
            return Err(CbdError::UnsupportedParameter);
        }
        let center_exact = r * self.center() * (BigRational::one() - self.center());
        let mantissa = round_to_nearest_even(&center_exact, &self.profile);
        let scaled_center = &center_exact * self.profile.scale();
        let quantization_inexact = !scaled_center.is_integer();
        let radius_as_rational =
            BigRational::from_integer(BigInt::from_biguint(Sign::Plus, self.radius.clone()));
        let propagated = ceil_rational(&(r * radius_as_rational))
            .to_biguint()
            .ok_or(CbdError::InvalidInterval)?;
        let uncapped = propagated + BigUint::from(quantization_inexact);
        let domain_radius = ceil_rational(
            &((&self.profile.maximum - &self.profile.minimum) * self.profile.scale()),
        )
        .to_biguint()
        .ok_or(CbdError::InvalidInterval)?;
        Self::from_parts(self.profile.clone(), mantissa, uncapped.min(domain_radius))
    }

    /// Bound local derivado da expansão exata de `x(1-x)` em torno do centro.
    ///
    /// Para `x=(N+d)/S`, `|d|<=R`, o erro do numerador de saída é limitado por
    /// `r*(R*|S-2N|+R²)/S`. Ele é mais caro que o bound global, mas acompanha
    /// a derivada local e evita crescimento artificial perto de `x=1/2`.
    pub fn logistic_step_local_bound(&self, r: &BigRational) -> Result<Self, CbdError> {
        if r.is_negative()
            || self.profile.minimum < BigRational::zero()
            || self.profile.maximum > BigRational::one()
        {
            return Err(CbdError::UnsupportedParameter);
        }
        let center_exact = r * self.center() * (BigRational::one() - self.center());
        let mantissa = round_to_nearest_even(&center_exact, &self.profile);
        let scaled_center = &center_exact * self.profile.scale();
        let quantization_inexact = !scaled_center.is_integer();

        let scale = self.profile.scale();
        let distance = (&scale - (&self.mantissa << 1_usize)).abs();
        let radius = BigInt::from_biguint(Sign::Plus, self.radius.clone());
        let error_numerator = &radius * distance + &radius * &radius;
        let propagated = ceil_rational(&(r * BigRational::new(error_numerator, scale.clone())))
            .to_biguint()
            .ok_or(CbdError::InvalidInterval)?;
        let uncapped = propagated + BigUint::from(quantization_inexact);
        let domain_radius =
            ceil_rational(&((&self.profile.maximum - &self.profile.minimum) * scale))
                .to_biguint()
                .ok_or(CbdError::InvalidInterval)?;
        Self::from_parts(self.profile.clone(), mantissa, uncapped.min(domain_radius))
    }

    /// Ablação sequencial do bound local.
    ///
    /// Quantiza `x*(1-x)` na grade do perfil e propaga seu raio antes de
    /// multiplicar pelo parâmetro público e quantizar novamente. O complemento
    /// `1-x` é exato na mesma grade diádica.
    pub fn logistic_step_sequential_local_bound(&self, r: &BigRational) -> Result<Self, CbdError> {
        if r.is_negative()
            || self.profile.minimum < BigRational::zero()
            || self.profile.maximum > BigRational::one()
        {
            return Err(CbdError::UnsupportedParameter);
        }

        let scale = self.profile.scale();
        let domain_radius =
            ceil_rational(&((&self.profile.maximum - &self.profile.minimum) * &scale))
                .to_biguint()
                .ok_or(CbdError::InvalidInterval)?;

        let product_exact = self.center() * (BigRational::one() - self.center());
        let product_mantissa = round_to_nearest_even(&product_exact, &self.profile);
        let product_scaled = &product_exact * &scale;
        let product_quantization_inexact = !product_scaled.is_integer();

        let distance = (&scale - (&self.mantissa << 1_usize)).abs();
        let radius = BigInt::from_biguint(Sign::Plus, self.radius.clone());
        let product_error_numerator = &radius * distance + &radius * &radius;
        let product_propagated =
            ceil_rational(&BigRational::new(product_error_numerator, scale.clone()))
                .to_biguint()
                .ok_or(CbdError::InvalidInterval)?;
        let product_radius = (product_propagated + BigUint::from(product_quantization_inexact))
            .min(domain_radius.clone());

        let product_center = BigRational::new(product_mantissa, scale.clone());
        let output_exact = r * product_center;
        let output_mantissa = round_to_nearest_even(&output_exact, &self.profile);
        let output_scaled = &output_exact * &scale;
        let output_quantization_inexact = !output_scaled.is_integer();

        let product_radius_rational =
            BigRational::from_integer(BigInt::from_biguint(Sign::Plus, product_radius));
        let output_propagated = ceil_rational(&(r * product_radius_rational))
            .to_biguint()
            .ok_or(CbdError::InvalidInterval)?;
        let output_radius =
            (output_propagated + BigUint::from(output_quantization_inexact)).min(domain_radius);

        Self::from_parts(self.profile.clone(), output_mantissa, output_radius)
    }

    pub fn ge_threshold(&self, threshold: &BigRational) -> Decision3 {
        if self.lower() >= *threshold {
            Decision3::True
        } else if self.upper() < *threshold {
            Decision3::False
        } else {
            Decision3::Indeterminate
        }
    }

    pub fn le_threshold(&self, threshold: &BigRational) -> Decision3 {
        if self.upper() <= *threshold {
            Decision3::True
        } else if self.lower() > *threshold {
            Decision3::False
        } else {
            Decision3::Indeterminate
        }
    }

    pub fn linear_score(bias: &Self, terms: &[(Self, Self)]) -> Result<Self, CbdError> {
        let mut accumulator = bias.clone();
        for (value, weight) in terms {
            accumulator = value.fma(weight, &accumulator)?;
        }
        Ok(accumulator)
    }

    pub fn escalate_exact(&self, exact: &BigRational) -> Result<Self, CbdError> {
        Self::from_exact(self.profile.next_profile()?, exact)
    }

    fn from_interval_with_center(
        profile: NumericProfile,
        center_hint: &BigRational,
        lower: &BigRational,
        upper: &BigRational,
    ) -> Result<Self, CbdError> {
        if lower > upper {
            return Err(CbdError::InvalidInterval);
        }
        if lower < profile.minimum() || upper > profile.maximum() {
            return Err(CbdError::OutsideDomain);
        }
        let mantissa = round_to_nearest_even(center_hint, &profile);
        let center = BigRational::new(mantissa.clone(), profile.scale());
        let radius = minimal_radius(&center, lower, upper, &profile)?;
        let value = Self {
            profile,
            mantissa,
            radius,
        };
        value.validate_domain()?;
        debug_assert!(value.is_canonical_for_interval(lower, upper));
        Ok(value)
    }

    fn ensure_same_profile(&self, rhs: &Self) -> Result<(), CbdError> {
        if self.profile != rhs.profile {
            return Err(CbdError::ProfileMismatch);
        }
        Ok(())
    }

    fn validate_domain(&self) -> Result<(), CbdError> {
        let magnitude_limit = BigInt::one() << self.profile.mantissa_magnitude_bits;
        if self.mantissa.abs() >= magnitude_limit {
            return Err(CbdError::OutsideDomain);
        }
        let center = self.center();
        if center < self.profile.minimum || center > self.profile.maximum {
            return Err(CbdError::OutsideDomain);
        }
        let domain_width = (&self.profile.maximum - &self.profile.minimum) * self.profile.scale();
        let maximum_radius = ceil_rational(&domain_width)
            .to_biguint()
            .ok_or(CbdError::OutsideDomain)?;
        if self.radius > maximum_radius {
            return Err(CbdError::OutsideDomain);
        }
        Ok(())
    }
}

fn round_to_nearest_even(value: &BigRational, profile: &NumericProfile) -> BigInt {
    let scaled_numerator = value.numer() * profile.scale();
    let negative = scaled_numerator.is_negative();
    let magnitude = scaled_numerator.abs();
    let denominator = value.denom();
    let quotient = &magnitude / denominator;
    let remainder = &magnitude % denominator;
    let doubled = &remainder << 1_usize;
    let round_up = doubled > *denominator
        || (doubled == *denominator && (&quotient & BigInt::one()) == BigInt::one());
    let selected = quotient
        + if round_up {
            BigInt::one()
        } else {
            BigInt::zero()
        };
    if negative {
        -selected
    } else {
        selected
    }
}

fn minimal_radius(
    center: &BigRational,
    lower: &BigRational,
    upper: &BigRational,
    profile: &NumericProfile,
) -> Result<BigUint, CbdError> {
    if lower > upper {
        return Err(CbdError::InvalidInterval);
    }
    let scale = profile.scale();
    let lower_distance = if center > lower {
        (center - lower) * &scale
    } else {
        BigRational::zero()
    };
    let upper_distance = if upper > center {
        (upper - center) * &scale
    } else {
        BigRational::zero()
    };
    let lower_radius = ceil_rational(&lower_distance);
    let upper_radius = ceil_rational(&upper_distance);
    lower_radius
        .max(upper_radius)
        .to_biguint()
        .ok_or(CbdError::InvalidInterval)
}

fn floor_rational(value: &BigRational) -> BigInt {
    let quotient = value.numer() / value.denom();
    let remainder = value.numer() % value.denom();
    if value.is_negative() && !remainder.is_zero() {
        quotient - BigInt::one()
    } else {
        quotient
    }
}

fn ceil_rational(value: &BigRational) -> BigInt {
    let quotient = value.numer() / value.denom();
    let remainder = value.numer() % value.denom();
    if value.is_positive() && !remainder.is_zero() {
        quotient + BigInt::one()
    } else {
        quotient
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_rounding_is_ties_to_even() {
        let profile = NumericProfile::new(
            "signed-tiny",
            1,
            2,
            4,
            BigRational::from_integer((-2).into()),
            BigRational::from_integer(2.into()),
            vec![BigRational::zero()],
            None,
        )
        .unwrap();
        let positive =
            CertifiedDyadic::from_exact(profile.clone(), &BigRational::new(1.into(), 8.into()))
                .unwrap();
        let negative =
            CertifiedDyadic::from_exact(profile, &BigRational::new((-1).into(), 8.into())).unwrap();
        assert_eq!(positive.mantissa(), &BigInt::zero());
        assert_eq!(negative.mantissa(), &BigInt::zero());
        assert_eq!(positive.radius(), &BigUint::one());
        assert_eq!(negative.radius(), &BigUint::one());
    }
}

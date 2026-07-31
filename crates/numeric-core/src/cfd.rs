//! Referência racional para Certified Floating Dyadic (CFD).
//!
//! O centro tem a forma dos números binários IEEE 754 normais, mas a relação
//! certificada é um intervalo em ULPs. O algoritmo honesto escolhe o centro
//! por round-to-nearest-ties-to-even; a segurança das decisões depende apenas
//! da inclusão do valor exato no intervalo.

use crate::{Decision3, OverflowPolicy};
use num_bigint::{BigInt, BigUint, Sign};
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CfdRoundingPolicy {
    HonestNearestTiesToEvenCertifiedEnclosure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CfdOperation {
    Add,
    Multiply,
    OneMinus,
    Fma,
    LogisticSequential,
    LogisticFused,
    StableAffine,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CfdProfile {
    id: String,
    version: u16,
    precision_bits: usize,
    exponent_lanes: Vec<i16>,
    radius_bits: usize,
    minimum: BigRational,
    maximum: BigRational,
    operations: Vec<CfdOperation>,
    thresholds: Vec<BigRational>,
    rounding: CfdRoundingPolicy,
    overflow: OverflowPolicy,
    next_profile_id: Option<String>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CfdError {
    #[error("perfil CFD inválido: {0}")]
    InvalidProfile(&'static str),
    #[error("valor CFD não canônico")]
    NonCanonical,
    #[error("valor ou intervalo fora do domínio público")]
    OutsideDomain,
    #[error("operação entre perfis diferentes")]
    ProfileMismatch,
    #[error("operação não autorizada pelo perfil")]
    OperationNotAllowed,
    #[error("intervalo inválido")]
    InvalidInterval,
    #[error("binary32 fora do subconjunto normal finito e +0")]
    UnsupportedBinary32,
    #[error("o perfil não contém o expoente binary32")]
    MissingExponentLane,
    #[error("o perfil seguinte não corresponde ao identificador comprometido")]
    InvalidNextProfile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertifiedFloat {
    profile: CfdProfile,
    sign: bool,
    exponent_lane: usize,
    significand: BigUint,
    radius: BigUint,
    zero: bool,
    wide: bool,
}

impl CfdProfile {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        version: u16,
        precision_bits: usize,
        mut exponent_lanes: Vec<i16>,
        radius_bits: usize,
        minimum: BigRational,
        maximum: BigRational,
        operations: Vec<CfdOperation>,
        thresholds: Vec<BigRational>,
        next_profile_id: Option<String>,
    ) -> Result<Self, CfdError> {
        if !(2..=126).contains(&precision_bits) {
            return Err(CfdError::InvalidProfile("p deve pertencer a 2..=126"));
        }
        if exponent_lanes.is_empty() {
            return Err(CfdError::InvalidProfile("corredor de expoente vazio"));
        }
        exponent_lanes.sort_unstable();
        exponent_lanes.dedup();
        if exponent_lanes.iter().any(|e| !(-126..=127).contains(e)) {
            return Err(CfdError::InvalidProfile("expoente fora da faixa binary32"));
        }
        if !(1..=126).contains(&radius_bits) {
            return Err(CfdError::InvalidProfile(
                "radius_bits deve pertencer a 1..=126",
            ));
        }
        if minimum >= maximum {
            return Err(CfdError::InvalidProfile("domínio vazio"));
        }
        if operations.is_empty() {
            return Err(CfdError::InvalidProfile("nenhuma operação autorizada"));
        }
        if thresholds
            .iter()
            .any(|threshold| threshold < &minimum || threshold > &maximum)
        {
            return Err(CfdError::InvalidProfile("limiar fora do domínio"));
        }
        Ok(Self {
            id: id.into(),
            version,
            precision_bits,
            exponent_lanes,
            radius_bits,
            minimum,
            maximum,
            operations,
            thresholds,
            rounding: CfdRoundingPolicy::HonestNearestTiesToEvenCertifiedEnclosure,
            overflow: OverflowPolicy::Reject,
            next_profile_id,
        })
    }

    pub fn bounded_unit(precision_bits: usize) -> Result<Self, CfdError> {
        Self::new(
            format!("cfd-unit-p{precision_bits}"),
            1,
            precision_bits,
            (-8..=0).collect(),
            16,
            BigRational::zero(),
            BigRational::one(),
            vec![
                CfdOperation::Add,
                CfdOperation::Multiply,
                CfdOperation::OneMinus,
                CfdOperation::Fma,
                CfdOperation::LogisticSequential,
                CfdOperation::LogisticFused,
            ],
            vec![
                BigRational::new(1.into(), 4.into()),
                BigRational::new(1.into(), 2.into()),
                BigRational::new(3.into(), 4.into()),
            ],
            None,
        )
    }

    pub fn stable_score(precision_bits: usize) -> Result<Self, CfdError> {
        Self::new(
            format!("cfd-stable-score-p{precision_bits}"),
            1,
            precision_bits,
            (-16..=1).collect(),
            16,
            BigRational::zero(),
            BigRational::from_integer(2.into()),
            vec![
                CfdOperation::Add,
                CfdOperation::Multiply,
                CfdOperation::Fma,
                CfdOperation::StableAffine,
            ],
            vec![BigRational::one()],
            None,
        )
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn precision_bits(&self) -> usize {
        self.precision_bits
    }
    pub fn exponent_lanes(&self) -> &[i16] {
        &self.exponent_lanes
    }
    pub fn radius_bits(&self) -> usize {
        self.radius_bits
    }
    pub fn minimum(&self) -> &BigRational {
        &self.minimum
    }
    pub fn maximum(&self) -> &BigRational {
        &self.maximum
    }
    pub fn operations(&self) -> &[CfdOperation] {
        &self.operations
    }
    pub fn thresholds(&self) -> &[BigRational] {
        &self.thresholds
    }
    pub fn rounding(&self) -> CfdRoundingPolicy {
        self.rounding
    }
    pub fn overflow(&self) -> OverflowPolicy {
        self.overflow
    }
    pub fn next_profile_id(&self) -> Option<&str> {
        self.next_profile_id.as_deref()
    }

    /// Serialização canônica independente de JSON/CBOR para o compromisso.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_bytes(&mut out, self.id.as_bytes());
        out.extend_from_slice(&self.version.to_be_bytes());
        out.extend_from_slice(&(self.precision_bits as u32).to_be_bytes());
        out.extend_from_slice(&(self.exponent_lanes.len() as u32).to_be_bytes());
        for lane in &self.exponent_lanes {
            out.extend_from_slice(&lane.to_be_bytes());
        }
        out.extend_from_slice(&(self.radius_bits as u32).to_be_bytes());
        put_rational(&mut out, &self.minimum);
        put_rational(&mut out, &self.maximum);
        out.extend_from_slice(&(self.operations.len() as u32).to_be_bytes());
        for operation in &self.operations {
            out.push(*operation as u8);
        }
        out.extend_from_slice(&(self.thresholds.len() as u32).to_be_bytes());
        for threshold in &self.thresholds {
            put_rational(&mut out, threshold);
        }
        out.push(self.rounding as u8);
        out.push(self.overflow as u8);
        match &self.next_profile_id {
            Some(id) => {
                out.push(1);
                put_bytes(&mut out, id.as_bytes());
            }
            None => out.push(0),
        }
        out
    }

    pub fn profile_hash(&self) -> [u8; 32] {
        Sha256::digest(self.canonical_bytes()).into()
    }

    fn allows(&self, operation: CfdOperation) -> bool {
        self.operations.contains(&operation)
    }

    fn maximum_radius(&self) -> BigUint {
        (BigUint::one() << self.radius_bits) - BigUint::one()
    }
}

impl CertifiedFloat {
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        profile: CfdProfile,
        sign: bool,
        exponent_lane: usize,
        significand: BigUint,
        radius: BigUint,
        zero: bool,
        wide: bool,
    ) -> Result<Self, CfdError> {
        let value = Self {
            profile,
            sign,
            exponent_lane,
            significand,
            radius,
            zero,
            wide,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn from_exact(profile: CfdProfile, exact: &BigRational) -> Result<Self, CfdError> {
        Self::from_interval_with_center(profile, exact, exact, exact)
    }

    pub fn wide(profile: CfdProfile) -> Self {
        Self {
            profile,
            sign: false,
            exponent_lane: 0,
            significand: BigUint::zero(),
            radius: BigUint::zero(),
            zero: false,
            wide: true,
        }
    }

    pub fn profile(&self) -> &CfdProfile {
        &self.profile
    }
    pub fn sign(&self) -> bool {
        self.sign
    }
    pub fn exponent_lane(&self) -> usize {
        self.exponent_lane
    }
    pub fn exponent(&self) -> i16 {
        self.profile.exponent_lanes[self.exponent_lane]
    }
    pub fn significand(&self) -> &BigUint {
        &self.significand
    }
    pub fn radius(&self) -> &BigUint {
        &self.radius
    }
    pub fn is_zero(&self) -> bool {
        self.zero
    }
    pub fn is_wide(&self) -> bool {
        self.wide
    }

    pub fn ulp(&self) -> BigRational {
        let exponent = if self.zero {
            self.profile.exponent_lanes[0]
        } else {
            self.exponent()
        } - (self.profile.precision_bits as i16 - 1);
        power_of_two(exponent)
    }

    pub fn center(&self) -> BigRational {
        if self.zero || self.wide {
            return BigRational::zero();
        }
        let magnitude =
            BigRational::from_integer(BigInt::from_biguint(Sign::Plus, self.significand.clone()))
                * self.ulp();
        if self.sign {
            -magnitude
        } else {
            magnitude
        }
    }

    pub fn lower(&self) -> BigRational {
        if self.wide {
            return self.profile.minimum.clone();
        }
        let radius =
            BigRational::from_integer(BigInt::from_biguint(Sign::Plus, self.radius.clone()))
                * self.ulp();
        (self.center() - radius).max(self.profile.minimum.clone())
    }

    pub fn upper(&self) -> BigRational {
        if self.wide {
            return self.profile.maximum.clone();
        }
        let radius =
            BigRational::from_integer(BigInt::from_biguint(Sign::Plus, self.radius.clone()))
                * self.ulp();
        (self.center() + radius).min(self.profile.maximum.clone())
    }

    pub fn contains(&self, exact: &BigRational) -> bool {
        self.lower() <= *exact && *exact <= self.upper()
    }

    pub fn ge_threshold(&self, threshold: &BigRational) -> Decision3 {
        if self.wide {
            Decision3::Indeterminate
        } else if self.lower() >= *threshold {
            Decision3::True
        } else if self.upper() < *threshold {
            Decision3::False
        } else {
            Decision3::Indeterminate
        }
    }

    pub fn add(&self, rhs: &Self) -> Result<Self, CfdError> {
        self.binary_operation(
            rhs,
            CfdOperation::Add,
            |a, b| a + b,
            |a, b| (a.lower() + b.lower(), a.upper() + b.upper()),
        )
    }

    pub fn mul(&self, rhs: &Self) -> Result<Self, CfdError> {
        self.binary_operation(
            rhs,
            CfdOperation::Multiply,
            |a, b| a * b,
            |a, b| {
                let products = [
                    a.lower() * b.lower(),
                    a.lower() * b.upper(),
                    a.upper() * b.lower(),
                    a.upper() * b.upper(),
                ];
                (
                    products.iter().min().expect("four products").clone(),
                    products.iter().max().expect("four products").clone(),
                )
            },
        )
    }

    pub fn mul_public(&self, factor: &BigRational) -> Result<Self, CfdError> {
        self.ensure_allowed(CfdOperation::Multiply)?;
        if self.wide {
            return Ok(Self::wide(self.profile.clone()));
        }
        let a = self.lower() * factor;
        let b = self.upper() * factor;
        Self::from_interval_with_center(
            self.profile.clone(),
            &(self.center() * factor),
            &a.clone().min(b.clone()),
            &a.max(b),
        )
    }

    pub fn one_minus(&self) -> Result<Self, CfdError> {
        self.ensure_allowed(CfdOperation::OneMinus)?;
        if self.wide {
            return Ok(Self::wide(self.profile.clone()));
        }
        Self::from_interval_with_center(
            self.profile.clone(),
            &(BigRational::one() - self.center()),
            &(BigRational::one() - self.upper()),
            &(BigRational::one() - self.lower()),
        )
    }

    pub fn fma(&self, multiplier: &Self, addend: &Self) -> Result<Self, CfdError> {
        self.ensure_same_profile(multiplier)?;
        self.ensure_same_profile(addend)?;
        self.ensure_allowed(CfdOperation::Fma)?;
        if self.wide || multiplier.wide || addend.wide {
            return Ok(Self::wide(self.profile.clone()));
        }
        let products = [
            self.lower() * multiplier.lower(),
            self.lower() * multiplier.upper(),
            self.upper() * multiplier.lower(),
            self.upper() * multiplier.upper(),
        ];
        let lower = products.iter().min().expect("four products").clone() + addend.lower();
        let upper = products.iter().max().expect("four products").clone() + addend.upper();
        let center = self.center() * multiplier.center() + addend.center();
        Self::from_interval_with_center(self.profile.clone(), &center, &lower, &upper)
    }

    pub fn stable_affine_step(
        &self,
        alpha: &BigRational,
        innovation: &Self,
    ) -> Result<Self, CfdError> {
        self.ensure_same_profile(innovation)?;
        self.ensure_allowed(CfdOperation::StableAffine)?;
        if self.wide || innovation.wide {
            return Ok(Self::wide(self.profile.clone()));
        }
        let left = self.lower() * alpha;
        let right = self.upper() * alpha;
        let lower = left.clone().min(right.clone()) + innovation.lower();
        let upper = left.max(right) + innovation.upper();
        let center = self.center() * alpha + innovation.center();
        Self::from_interval_with_center(self.profile.clone(), &center, &lower, &upper)
    }

    pub fn logistic_step_sequential(&self, r: &BigRational) -> Result<Self, CfdError> {
        self.ensure_allowed(CfdOperation::LogisticSequential)?;
        if r.is_negative() {
            return Err(CfdError::OutsideDomain);
        }
        let product = self.logistic_product_sequential()?;
        match product.mul_public(r) {
            Ok(value) => Ok(value),
            Err(CfdError::OutsideDomain) => Ok(Self::wide(self.profile.clone())),
            Err(error) => Err(error),
        }
    }

    pub fn logistic_product_sequential(&self) -> Result<Self, CfdError> {
        self.ensure_allowed(CfdOperation::LogisticSequential)?;
        if self.wide {
            return Ok(Self::wide(self.profile.clone()));
        }
        if self.profile.minimum < BigRational::zero() || self.profile.maximum > BigRational::one() {
            return Err(CfdError::OutsideDomain);
        }
        // Preserva a dependência entre x e 1-x; multiplicação intervalar
        // independente produziria um bound espúrio acima de 1/4.
        let parabola = |x: &BigRational| x * (BigRational::one() - x);
        let a = self.lower();
        let b = self.upper();
        let fa = parabola(&a);
        let fb = parabola(&b);
        let lower = fa.clone().min(fb.clone());
        let mut upper = fa.max(fb);
        let half = BigRational::new(1.into(), 2.into());
        if a <= half && half <= b {
            upper = upper.max(BigRational::new(1.into(), 4.into()));
        }
        match Self::from_interval_with_center(
            self.profile.clone(),
            &parabola(&self.center()),
            &lower,
            &upper,
        ) {
            Ok(value) => Ok(value),
            Err(CfdError::OutsideDomain) => Ok(Self::wide(self.profile.clone())),
            Err(error) => Err(error),
        }
    }

    pub fn logistic_step_fused(&self, r: &BigRational) -> Result<Self, CfdError> {
        self.ensure_allowed(CfdOperation::LogisticFused)?;
        if self.wide {
            return Ok(Self::wide(self.profile.clone()));
        }
        if r.is_negative()
            || self.profile.minimum < BigRational::zero()
            || self.profile.maximum > BigRational::one()
        {
            return Err(CfdError::OutsideDomain);
        }
        let f = |x: &BigRational| r * x * (BigRational::one() - x);
        let a = self.lower();
        let b = self.upper();
        let fa = f(&a);
        let fb = f(&b);
        let lower = fa.clone().min(fb.clone());
        let mut upper = fa.max(fb);
        let half = BigRational::new(1.into(), 2.into());
        if a <= half && half <= b {
            upper = upper.max(f(&half));
        }
        match Self::from_interval_with_center(
            self.profile.clone(),
            &f(&self.center()),
            &lower,
            &upper,
        ) {
            Ok(value) => Ok(value),
            Err(CfdError::OutsideDomain) => Ok(Self::wide(self.profile.clone())),
            Err(error) => Err(error),
        }
    }

    pub fn escalate_exact(
        &self,
        next_profile: CfdProfile,
        exact: &BigRational,
    ) -> Result<Self, CfdError> {
        if self.profile.next_profile_id() != Some(next_profile.id()) {
            return Err(CfdError::InvalidNextProfile);
        }
        Self::from_exact(next_profile, exact)
    }

    pub fn from_binary32_bits(profile: CfdProfile, bits: u32) -> Result<Self, CfdError> {
        if profile.precision_bits != 24 {
            return Err(CfdError::UnsupportedBinary32);
        }
        let sign = bits >> 31 != 0;
        let biased = ((bits >> 23) & 0xff) as i16;
        let fraction = bits & 0x7f_ffff;
        if biased == 0 {
            if sign || fraction != 0 {
                return Err(CfdError::UnsupportedBinary32);
            }
            return Self::from_parts(
                profile,
                false,
                0,
                BigUint::zero(),
                BigUint::zero(),
                true,
                false,
            );
        }
        if biased == 255 {
            return Err(CfdError::UnsupportedBinary32);
        }
        let exponent = biased - 127;
        let lane = profile
            .exponent_lanes
            .binary_search(&exponent)
            .map_err(|_| CfdError::MissingExponentLane)?;
        Self::from_parts(
            profile,
            sign,
            lane,
            BigUint::from((1_u32 << 23) | fraction),
            BigUint::zero(),
            false,
            false,
        )
    }

    pub fn to_binary32_bits(&self) -> Result<u32, CfdError> {
        if self.profile.precision_bits != 24 || self.wide || !self.radius.is_zero() {
            return Err(CfdError::UnsupportedBinary32);
        }
        if self.zero {
            return Ok(0);
        }
        let biased = i32::from(self.exponent()) + 127;
        if !(1..=254).contains(&biased) {
            return Err(CfdError::UnsupportedBinary32);
        }
        let significand =
            u32::try_from(self.significand.clone()).map_err(|_| CfdError::UnsupportedBinary32)?;
        let fraction = significand - (1_u32 << 23);
        Ok((u32::from(self.sign) << 31) | ((biased as u32) << 23) | fraction)
    }

    fn binary_operation<FC, FI>(
        &self,
        rhs: &Self,
        operation: CfdOperation,
        center_fn: FC,
        interval_fn: FI,
    ) -> Result<Self, CfdError>
    where
        FC: Fn(&BigRational, &BigRational) -> BigRational,
        FI: Fn(&Self, &Self) -> (BigRational, BigRational),
    {
        self.ensure_same_profile(rhs)?;
        self.ensure_allowed(operation)?;
        if self.wide || rhs.wide {
            return Ok(Self::wide(self.profile.clone()));
        }
        let center = center_fn(&self.center(), &rhs.center());
        let (lower, upper) = interval_fn(self, rhs);
        Self::from_interval_with_center(self.profile.clone(), &center, &lower, &upper)
    }

    fn from_interval_with_center(
        profile: CfdProfile,
        center_hint: &BigRational,
        lower: &BigRational,
        upper: &BigRational,
    ) -> Result<Self, CfdError> {
        if lower > upper {
            return Err(CfdError::InvalidInterval);
        }
        if lower < profile.minimum() || upper > profile.maximum() {
            return Err(CfdError::OutsideDomain);
        }
        let center = honest_center_parts(&profile, center_hint)?;
        let Some((sign, lane, significand, zero, ulp)) = center else {
            return Ok(Self::wide(profile));
        };
        let center_value = if zero {
            BigRational::zero()
        } else {
            let magnitude =
                BigRational::from_integer(BigInt::from_biguint(Sign::Plus, significand.clone()))
                    * &ulp;
            if sign {
                -magnitude
            } else {
                magnitude
            }
        };
        let left = if center_value > *lower {
            (&center_value - lower) / &ulp
        } else {
            BigRational::zero()
        };
        let right = if *upper > center_value {
            (upper - &center_value) / &ulp
        } else {
            BigRational::zero()
        };
        let radius = ceil_rational(&left.max(right))
            .to_biguint()
            .ok_or(CfdError::InvalidInterval)?;
        if radius > profile.maximum_radius() {
            return Ok(Self::wide(profile));
        }
        Self::from_parts(profile, sign, lane, significand, radius, zero, false)
    }

    fn validate(&self) -> Result<(), CfdError> {
        if self.wide {
            if self.sign
                || self.zero
                || self.exponent_lane != 0
                || !self.significand.is_zero()
                || !self.radius.is_zero()
            {
                return Err(CfdError::NonCanonical);
            }
            return Ok(());
        }
        if self.radius > self.profile.maximum_radius() {
            return Err(CfdError::NonCanonical);
        }
        if self.zero {
            if self.sign || self.exponent_lane != 0 || !self.significand.is_zero() {
                return Err(CfdError::NonCanonical);
            }
        } else {
            if self.exponent_lane >= self.profile.exponent_lanes.len() {
                return Err(CfdError::NonCanonical);
            }
            let minimum = BigUint::one() << (self.profile.precision_bits - 1);
            let maximum = BigUint::one() << self.profile.precision_bits;
            if self.significand < minimum || self.significand >= maximum {
                return Err(CfdError::NonCanonical);
            }
        }
        if self.lower() > self.upper()
            || self.center() < self.profile.minimum
            || self.center() > self.profile.maximum
        {
            return Err(CfdError::OutsideDomain);
        }
        Ok(())
    }

    fn ensure_same_profile(&self, rhs: &Self) -> Result<(), CfdError> {
        if self.profile != rhs.profile {
            return Err(CfdError::ProfileMismatch);
        }
        Ok(())
    }

    fn ensure_allowed(&self, operation: CfdOperation) -> Result<(), CfdError> {
        if self.profile.allows(operation) {
            Ok(())
        } else {
            Err(CfdError::OperationNotAllowed)
        }
    }
}

type HonestCenter = Option<(bool, usize, BigUint, bool, BigRational)>;

fn honest_center_parts(
    profile: &CfdProfile,
    value: &BigRational,
) -> Result<HonestCenter, CfdError> {
    if value < profile.minimum() || value > profile.maximum() {
        return Err(CfdError::OutsideDomain);
    }
    if value.is_zero() {
        return Ok(Some((
            false,
            0,
            BigUint::zero(),
            true,
            power_of_two(profile.exponent_lanes[0] - (profile.precision_bits as i16 - 1)),
        )));
    }
    let sign = value.is_negative();
    let magnitude = value.abs();
    let minimum_sig = BigUint::one() << (profile.precision_bits - 1);
    let maximum_sig = BigUint::one() << profile.precision_bits;
    let mut best: Option<(BigRational, bool, usize, BigUint, BigRational)> = None;
    for (lane, exponent) in profile.exponent_lanes.iter().copied().enumerate() {
        let ulp = power_of_two(exponent - (profile.precision_bits as i16 - 1));
        let rounded = round_positive_rational_to_even(&(&magnitude / &ulp));
        let Some(significand) = rounded.to_biguint() else {
            continue;
        };
        if significand < minimum_sig || significand >= maximum_sig {
            continue;
        }
        let represented = BigRational::from_integer(rounded) * &ulp;
        let error = (&represented - &magnitude).abs();
        let replace = best.as_ref().map_or(true, |current| {
            error < current.0
                || (error == current.0
                    && (&significand & BigUint::one()).is_zero()
                    && !(&current.3 & BigUint::one()).is_zero())
        });
        if replace {
            best = Some((error, sign, lane, significand, ulp));
        }
    }
    Ok(Some(best.map_or_else(
        || {
            (
                false,
                0,
                BigUint::zero(),
                true,
                power_of_two(profile.exponent_lanes[0] - (profile.precision_bits as i16 - 1)),
            )
        },
        |(_, sign, lane, significand, ulp)| (sign, lane, significand, false, ulp),
    )))
}

fn round_positive_rational_to_even(value: &BigRational) -> BigInt {
    debug_assert!(!value.is_negative());
    let quotient = value.numer() / value.denom();
    let remainder = value.numer() % value.denom();
    let doubled = &remainder << 1_usize;
    let up = doubled > *value.denom()
        || (doubled == *value.denom() && (&quotient & BigInt::one()) == BigInt::one());
    quotient + if up { BigInt::one() } else { BigInt::zero() }
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

fn power_of_two(exponent: i16) -> BigRational {
    if exponent >= 0 {
        BigRational::from_integer(BigInt::one() << exponent as usize)
    } else {
        BigRational::new(
            BigInt::one(),
            BigInt::one() << usize::from(exponent.unsigned_abs()),
        )
    }
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn put_rational(out: &mut Vec<u8>, value: &BigRational) {
    let (sign, numerator) = value.numer().to_bytes_be();
    out.push(match sign {
        Sign::Minus => 1,
        Sign::NoSign => 0,
        Sign::Plus => 2,
    });
    put_bytes(out, &numerator);
    let (_, denominator) = value.denom().to_bytes_be();
    put_bytes(out, &denominator);
}

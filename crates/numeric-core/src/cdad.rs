//! Certified Decision-Adaptive Dyadic (CDAD).
//!
//! CDAD keeps CBD's certified interval semantics and makes the precision ladder
//! and retry policy public. The controller affects performance only: every
//! definitive answer is still derived from the certified interval predicate.

use crate::{CbdError, CertifiedDyadic, Decision3, NumericProfile};
use num_bigint::{BigInt, BigUint, Sign};
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrecisionPolicy {
    Sequential,
    MarginDirected,
    Fixed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommitmentPolicy {
    ExistingPoseidonLink,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionReason {
    Initial,
    SequentialRetry,
    MarginPrediction,
    MaximumPrecision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionAdaptiveProfile {
    id: String,
    version: u16,
    precision_ladder: Vec<usize>,
    minimum: BigRational,
    maximum: BigRational,
    thresholds: Vec<BigRational>,
    precision_policy: PrecisionPolicy,
    commitment_policy: CommitmentPolicy,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CdadError {
    #[error("perfil CDAD invalido: {0}")]
    InvalidProfile(&'static str),
    #[error("precisao {0} nao pertence a escada publica")]
    PrecisionOutsideLadder(usize),
    #[error("limiar nao pertence ao perfil CDAD")]
    ThresholdOutsideProfile,
    #[error("o valor CBD interno pertence a outro perfil")]
    InnerProfileMismatch,
    #[error(transparent)]
    Cbd(#[from] CbdError),
}

/// The concrete `(M,R,F,profile_id)` state proposed by CDAD.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertifiedAdaptiveDyadic {
    profile: DecisionAdaptiveProfile,
    value: CertifiedDyadic,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrecisionSelection {
    pub attempted_bits: usize,
    pub next_bits: Option<usize>,
    pub decision: Decision3,
    pub reason: SelectionReason,
    /// `|center-threshold|` measured in ULPs of the attempted precision.
    pub margin_ulps: BigRational,
    pub radius_ulps: BigUint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdaptiveDecision {
    pub value: CertifiedAdaptiveDyadic,
    pub decision: Decision3,
    pub attempts: Vec<PrecisionSelection>,
}

impl PrecisionPolicy {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::MarginDirected => "margin-directed",
            Self::Fixed => "fixed",
        }
    }
}

impl DecisionAdaptiveProfile {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        version: u16,
        precision_ladder: Vec<usize>,
        minimum: BigRational,
        maximum: BigRational,
        thresholds: Vec<BigRational>,
        precision_policy: PrecisionPolicy,
        commitment_policy: CommitmentPolicy,
    ) -> Result<Self, CdadError> {
        if precision_ladder.is_empty() {
            return Err(CdadError::InvalidProfile("escada de precisao vazia"));
        }
        if precision_ladder
            .iter()
            .any(|bits| *bits == 0 || *bits > 126)
        {
            return Err(CdadError::InvalidProfile("precisoes fora de 1..=126"));
        }
        if precision_ladder.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(CdadError::InvalidProfile(
                "escada deve ser estritamente crescente",
            ));
        }
        if minimum >= maximum {
            return Err(CdadError::InvalidProfile("dominio vazio"));
        }
        if thresholds.is_empty() {
            return Err(CdadError::InvalidProfile("nenhum limiar declarado"));
        }
        if thresholds
            .iter()
            .any(|threshold| threshold < &minimum || threshold > &maximum)
        {
            return Err(CdadError::InvalidProfile("limiar fora do dominio"));
        }
        let mut thresholds = thresholds;
        thresholds.sort();
        thresholds.dedup();
        Ok(Self {
            id: id.into(),
            version,
            precision_ladder,
            minimum,
            maximum,
            thresholds,
            precision_policy,
            commitment_policy,
        })
    }

    pub fn bounded_unit(precision_policy: PrecisionPolicy) -> Result<Self, CdadError> {
        Self::new(
            format!("cdad-unit-{}", precision_policy.tag()),
            1,
            vec![16, 32, 48, 64],
            BigRational::zero(),
            BigRational::one(),
            vec![
                BigRational::new(1.into(), 4.into()),
                BigRational::new(1.into(), 2.into()),
                BigRational::new(3.into(), 4.into()),
            ],
            precision_policy,
            CommitmentPolicy::ExistingPoseidonLink,
        )
    }

    pub fn signed_score(
        precision_policy: PrecisionPolicy,
        thresholds: Vec<BigRational>,
    ) -> Result<Self, CdadError> {
        Self::new(
            format!("cdad-signed-score-{}", precision_policy.tag()),
            1,
            vec![16, 32, 48, 64],
            BigRational::from_integer((-64).into()),
            BigRational::from_integer(64.into()),
            thresholds,
            precision_policy,
            CommitmentPolicy::ExistingPoseidonLink,
        )
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn precision_ladder(&self) -> &[usize] {
        &self.precision_ladder
    }
    pub fn minimum(&self) -> &BigRational {
        &self.minimum
    }
    pub fn maximum(&self) -> &BigRational {
        &self.maximum
    }
    pub fn thresholds(&self) -> &[BigRational] {
        &self.thresholds
    }
    pub fn precision_policy(&self) -> PrecisionPolicy {
        self.precision_policy
    }
    pub fn commitment_policy(&self) -> CommitmentPolicy {
        self.commitment_policy
    }

    pub fn numeric_profile(&self, bits: usize) -> Result<NumericProfile, CdadError> {
        let position = self
            .precision_ladder
            .iter()
            .position(|candidate| *candidate == bits)
            .ok_or(CdadError::PrecisionOutsideLadder(bits))?;
        NumericProfile::new(
            format!("{}-f{bits}", self.id),
            self.version,
            bits,
            bits + 8,
            self.minimum.clone(),
            self.maximum.clone(),
            self.thresholds.clone(),
            self.precision_ladder.get(position + 1).copied(),
        )
        .map_err(CdadError::from)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_bytes(&mut out, self.id.as_bytes());
        out.extend_from_slice(&self.version.to_be_bytes());
        out.extend_from_slice(&(self.precision_ladder.len() as u32).to_be_bytes());
        for bits in &self.precision_ladder {
            out.extend_from_slice(&(*bits as u32).to_be_bytes());
        }
        put_rational(&mut out, &self.minimum);
        put_rational(&mut out, &self.maximum);
        out.extend_from_slice(&(self.thresholds.len() as u32).to_be_bytes());
        for threshold in &self.thresholds {
            put_rational(&mut out, threshold);
        }
        out.push(self.precision_policy as u8);
        out.push(self.commitment_policy as u8);
        out
    }

    pub fn profile_hash(&self) -> [u8; 32] {
        Sha256::digest(self.canonical_bytes()).into()
    }

    /// Recompute one committed opportunity until it is definitive or the
    /// public ladder is exhausted. The closure must recompute from the same
    /// input; it must not merely refine the previous rounded center.
    pub fn evaluate<F>(
        &self,
        threshold: &BigRational,
        mut evaluate_at: F,
    ) -> Result<AdaptiveDecision, CdadError>
    where
        F: FnMut(&NumericProfile) -> Result<CertifiedDyadic, CbdError>,
    {
        if !self.thresholds.contains(threshold) {
            return Err(CdadError::ThresholdOutsideProfile);
        }
        let mut index = 0_usize;
        let mut attempts = Vec::new();
        loop {
            let bits = self.precision_ladder[index];
            let numeric_profile = self.numeric_profile(bits)?;
            let value = evaluate_at(&numeric_profile)?;
            if value.profile() != &numeric_profile {
                return Err(CdadError::InnerProfileMismatch);
            }
            let wrapped = CertifiedAdaptiveDyadic::from_cbd(self.clone(), value)?;
            let decision = wrapped.ge_threshold(threshold);
            let margin_ulps = wrapped.margin_ulps(threshold);
            let radius_ulps = wrapped.radius().clone();
            let mut next_index = (decision == Decision3::Indeterminate)
                .then(|| self.next_index(index, &margin_ulps, &radius_ulps))
                .flatten();
            if decision == Decision3::Indeterminate
                && self.precision_policy == PrecisionPolicy::MarginDirected
            {
                if let Some(start) = next_index {
                    let last = self.precision_ladder.len() - 1;
                    let mut selected = last;
                    // Speculative certified evaluations are local prover work,
                    // not proof attempts. They may select a rung but cannot
                    // authorize a decision; the selected rung is recomputed
                    // and constrained on the next loop iteration.
                    for candidate in start..=last {
                        let preview_profile =
                            self.numeric_profile(self.precision_ladder[candidate])?;
                        let preview = evaluate_at(&preview_profile)?;
                        if preview.profile() != &preview_profile {
                            return Err(CdadError::InnerProfileMismatch);
                        }
                        selected = candidate;
                        if preview.ge_threshold(threshold) != Decision3::Indeterminate {
                            break;
                        }
                    }
                    next_index = Some(selected);
                }
            }
            let next_bits = next_index.map(|position| self.precision_ladder[position]);
            let reason = if attempts.is_empty() {
                SelectionReason::Initial
            } else if next_bits.is_none() {
                SelectionReason::MaximumPrecision
            } else if self.precision_policy == PrecisionPolicy::MarginDirected {
                SelectionReason::MarginPrediction
            } else {
                SelectionReason::SequentialRetry
            };
            attempts.push(PrecisionSelection {
                attempted_bits: bits,
                next_bits,
                decision,
                reason,
                margin_ulps,
                radius_ulps,
            });
            match next_index {
                Some(position) => index = position,
                None => {
                    return Ok(AdaptiveDecision {
                        value: wrapped,
                        decision,
                        attempts,
                    })
                }
            }
        }
    }

    fn next_index(
        &self,
        current: usize,
        margin_ulps: &BigRational,
        radius_ulps: &BigUint,
    ) -> Option<usize> {
        if self.precision_policy == PrecisionPolicy::Fixed {
            return None;
        }
        let sequential = (current + 1 < self.precision_ladder.len()).then_some(current + 1);
        if self.precision_policy == PrecisionPolicy::Sequential {
            return sequential;
        }
        // A center exactly on the threshold provides no finite margin from
        // which to estimate a rung.  Retrying intermediate profiles cannot be
        // justified by the predictor, so the directed policy jumps to the
        // strongest public profile and lets its enclosure decide.
        if margin_ulps.is_zero() {
            return self
                .precision_ladder
                .len()
                .checked_sub(1)
                .filter(|last| *last > current);
        }
        let guarded_radius = BigRational::from_integer(BigInt::from_biguint(
            Sign::Plus,
            radius_ulps + BigUint::one(),
        ));
        let current_bits = self.precision_ladder[current];
        self.precision_ladder
            .iter()
            .enumerate()
            .skip(current + 1)
            .find_map(|(index, bits)| {
                let shrink = BigInt::one() << (*bits - current_bits);
                (&guarded_radius / shrink < *margin_ulps).then_some(index)
            })
            .or_else(|| {
                self.precision_ladder
                    .len()
                    .checked_sub(1)
                    .filter(|last| *last > current)
            })
    }
}

impl CertifiedAdaptiveDyadic {
    pub fn from_exact(
        profile: DecisionAdaptiveProfile,
        fractional_bits: usize,
        exact: &BigRational,
    ) -> Result<Self, CdadError> {
        let numeric = profile.numeric_profile(fractional_bits)?;
        Self::from_cbd(profile, CertifiedDyadic::from_exact(numeric, exact)?)
    }

    pub fn from_parts(
        profile: DecisionAdaptiveProfile,
        fractional_bits: usize,
        mantissa: BigInt,
        radius: BigUint,
    ) -> Result<Self, CdadError> {
        let numeric = profile.numeric_profile(fractional_bits)?;
        Self::from_cbd(
            profile,
            CertifiedDyadic::from_parts(numeric, mantissa, radius)?,
        )
    }

    pub fn from_cbd(
        profile: DecisionAdaptiveProfile,
        value: CertifiedDyadic,
    ) -> Result<Self, CdadError> {
        let expected = profile.numeric_profile(value.profile().fractional_bits())?;
        if value.profile() != &expected {
            return Err(CdadError::InnerProfileMismatch);
        }
        Ok(Self { profile, value })
    }

    pub fn profile(&self) -> &DecisionAdaptiveProfile {
        &self.profile
    }
    pub fn fractional_bits(&self) -> usize {
        self.value.profile().fractional_bits()
    }
    pub fn mantissa(&self) -> &BigInt {
        self.value.mantissa()
    }
    pub fn radius(&self) -> &BigUint {
        self.value.radius()
    }
    pub fn center(&self) -> BigRational {
        self.value.center()
    }
    pub fn lower(&self) -> BigRational {
        self.value.lower()
    }
    pub fn upper(&self) -> BigRational {
        self.value.upper()
    }
    pub fn contains(&self, exact: &BigRational) -> bool {
        self.value.contains(exact)
    }
    pub fn ge_threshold(&self, threshold: &BigRational) -> Decision3 {
        self.value.ge_threshold(threshold)
    }
    pub fn margin_ulps(&self, threshold: &BigRational) -> BigRational {
        (self.center() - threshold).abs() * BigRational::from_integer(self.value.profile().scale())
    }
    pub fn into_cbd(self) -> CertifiedDyadic {
        self.value
    }
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn put_rational(out: &mut Vec<u8>, value: &BigRational) {
    let (sign, numerator) = value.numer().to_bytes_be();
    out.push(match sign {
        Sign::Minus => 0,
        Sign::NoSign => 1,
        Sign::Plus => 2,
    });
    put_bytes(out, &numerator);
    let (_, denominator) = value.denom().to_bytes_be();
    put_bytes(out, &denominator);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn half() -> BigRational {
        BigRational::new(1.into(), 2.into())
    }

    #[test]
    fn state_exposes_m_r_f_and_contains_reference() {
        let profile = DecisionAdaptiveProfile::bounded_unit(PrecisionPolicy::Sequential).unwrap();
        let exact = BigRational::new(1.into(), 3.into());
        let value = CertifiedAdaptiveDyadic::from_exact(profile, 16, &exact).unwrap();
        assert_eq!(value.fractional_bits(), 16);
        assert!(value.contains(&exact));
        assert_eq!(value.ge_threshold(&half()), Decision3::False);
    }

    #[test]
    fn fixed_policy_never_retries() {
        let profile = DecisionAdaptiveProfile::bounded_unit(PrecisionPolicy::Fixed).unwrap();
        let result = profile
            .evaluate(&half(), |numeric| {
                CertifiedDyadic::from_parts(numeric.clone(), BigInt::one() << 15, 1_u32.into())
            })
            .unwrap();
        assert_eq!(result.decision, Decision3::Indeterminate);
        assert_eq!(result.attempts.len(), 1);
    }

    #[test]
    fn sequential_policy_recomputes_every_rung() {
        let profile = DecisionAdaptiveProfile::bounded_unit(PrecisionPolicy::Sequential).unwrap();
        let mut visited = Vec::new();
        let result = profile
            .evaluate(&half(), |numeric| {
                visited.push(numeric.fractional_bits());
                let bits = numeric.fractional_bits();
                CertifiedDyadic::from_parts(
                    numeric.clone(),
                    BigInt::one() << (bits - 1),
                    BigUint::from(if bits < 64 { 2_u32 } else { 0_u32 }),
                )
            })
            .unwrap();
        assert_eq!(visited, vec![16, 32, 48, 64]);
        assert_eq!(result.decision, Decision3::True);
    }

    #[test]
    fn margin_policy_can_skip_rungs_and_recertifies() {
        let threshold = &half() + BigRational::new(1.into(), (3_i64 << 16).into());
        let profile = DecisionAdaptiveProfile::new(
            "directed-test",
            1,
            vec![16, 32, 48, 64],
            BigRational::zero(),
            BigRational::one(),
            vec![threshold.clone()],
            PrecisionPolicy::MarginDirected,
            CommitmentPolicy::ExistingPoseidonLink,
        )
        .unwrap();
        let mut visited = Vec::new();
        let result = profile
            .evaluate(&threshold, |numeric| {
                visited.push(numeric.fractional_bits());
                let bits = numeric.fractional_bits();
                CertifiedDyadic::from_parts(
                    numeric.clone(),
                    BigInt::one() << (bits - 1),
                    if bits < 64 {
                        40_000_u32.into()
                    } else {
                        BigUint::zero()
                    },
                )
            })
            .unwrap();
        assert!(visited.starts_with(&[16, 48]));
        assert_eq!(
            result
                .attempts
                .iter()
                .map(|attempt| attempt.attempted_bits)
                .collect::<Vec<_>>(),
            vec![16, 48]
        );
        assert_eq!(result.decision, Decision3::False);
        assert_eq!(result.attempts[0].next_bits, Some(48));
    }

    #[test]
    fn hashes_commit_to_policy() {
        let sequential =
            DecisionAdaptiveProfile::bounded_unit(PrecisionPolicy::Sequential).unwrap();
        let directed =
            DecisionAdaptiveProfile::bounded_unit(PrecisionPolicy::MarginDirected).unwrap();
        assert_ne!(sequential.profile_hash(), directed.profile_hash());
    }
}

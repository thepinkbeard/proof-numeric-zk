//! Public, serializable numeric semantics and error contracts.
//!
//! This deliberately does not replace the CBD `NumericProfile`.  The latter
//! configures one certified representation; this type describes the complete
//! numeric contract of a primitive-function experiment.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, Zero};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

pub const NUMERIC_CONTRACT_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct RationalSpec {
    pub numerator: String,
    pub denominator: String,
}

impl RationalSpec {
    pub fn new(
        numerator: impl Into<BigInt>,
        denominator: impl Into<BigInt>,
    ) -> Result<Self, ContractError> {
        let denominator = denominator.into();
        if denominator.is_zero() {
            return Err(ContractError::ZeroDenominator);
        }
        Ok(Self::from_rational(&BigRational::new(
            numerator.into(),
            denominator,
        )))
    }

    pub fn integer(value: impl Into<BigInt>) -> Self {
        Self::from_rational(&BigRational::from_integer(value.into()))
    }

    pub fn from_rational(value: &BigRational) -> Self {
        Self {
            numerator: value.numer().to_string(),
            denominator: value.denom().to_string(),
        }
    }

    pub fn to_rational(&self) -> Result<BigRational, ContractError> {
        let numerator = self
            .numerator
            .parse::<BigInt>()
            .map_err(|_| ContractError::InvalidRational)?;
        let denominator = self
            .denominator
            .parse::<BigInt>()
            .map_err(|_| ContractError::InvalidRational)?;
        if denominator.is_zero() {
            return Err(ContractError::ZeroDenominator);
        }
        let canonical = BigRational::new(numerator, denominator);
        if &Self::from_rational(&canonical) != self {
            return Err(ContractError::NonCanonicalRational);
        }
        Ok(canonical)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NumericRepresentationKind {
    MpfrReference,
    Binary32Reference,
    SignedFixedPoint,
    Quantized,
    ApproximateFloatingPoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepresentationSpec {
    pub kind: NumericRepresentationKind,
    pub signed: bool,
    pub integer_bits: u16,
    pub fractional_bits: u16,
    /// The scale is exactly `2^scale_power`.
    pub scale_power: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuantizationRounding {
    Floor,
    TowardZero,
    NearestTiesToEven,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RescalingMode {
    AfterEveryMultiplication,
    AtExpressionBoundary,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverflowMode {
    Reject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnderflowMode {
    QuantizeToGrid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExceptionalValuePolicy {
    RejectNonFinite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationSemantics {
    pub rounding: QuantizationRounding,
    pub rescaling: RescalingMode,
    pub division_rounding: QuantizationRounding,
    pub overflow: OverflowMode,
    pub underflow: UnderflowMode,
    pub exceptional_values: ExceptionalValuePolicy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ValidDomain {
    pub minimum: RationalSpec,
    pub maximum: RationalSpec,
    pub minimum_inclusive: bool,
    pub maximum_inclusive: bool,
}

impl ValidDomain {
    pub fn closed(minimum: &BigRational, maximum: &BigRational) -> Self {
        Self {
            minimum: RationalSpec::from_rational(minimum),
            maximum: RationalSpec::from_rational(maximum),
            minimum_inclusive: true,
            maximum_inclusive: true,
        }
    }

    pub fn bounds(&self) -> Result<(BigRational, BigRational), ContractError> {
        Ok((self.minimum.to_rational()?, self.maximum.to_rational()?))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FunctionId {
    Exp,
    Sigmoid,
    Tanh,
    FractionalPower,
    Cos,
}

impl FunctionId {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Exp => "exp",
            Self::Sigmoid => "sigmoid",
            Self::Tanh => "tanh",
            Self::FractionalPower => "fractional-power",
            Self::Cos => "cos",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApproximationMethod {
    None,
    Taylor,
    PadeDiagonal,
    GaussLegendreImplicit,
}

impl ApproximationMethod {
    pub fn tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Taylor => "taylor",
            Self::PadeDiagonal => "pade-diagonal",
            Self::GaussLegendreImplicit => "gauss-legendre-implicit",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApproximationSpec {
    pub function: FunctionId,
    pub method: ApproximationMethod,
    pub method_version: u16,
    pub order: u16,
    pub expansion_center: RationalSpec,
    pub evaluation_form: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, RationalSpec>,
    pub coefficient_or_node_source: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    PaperDerived,
    ArtifactDerived,
    RepositoryDerived,
    OurExtension,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceLevel {
    Formal,
    Analytical,
    ExhaustiveFiniteDomain,
    ObservedGrid,
    ModelAssumption,
}

impl EvidenceLevel {
    pub fn is_certifying(self) -> bool {
        matches!(
            self,
            Self::Formal | Self::Analytical | Self::ExhaustiveFiniteDomain
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BoundRule {
    None,
    Absolute {
        value: RationalSpec,
    },
    Named {
        rule_id: String,
        #[serde(default)]
        parameters: BTreeMap<String, RationalSpec>,
    },
}

impl BoundRule {
    fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::None => Ok(()),
            Self::Absolute { value } => {
                if value.to_rational()?.is_negative() {
                    Err(ContractError::NegativeErrorBound)
                } else {
                    Ok(())
                }
            }
            Self::Named {
                rule_id,
                parameters,
            } => {
                if rule_id.trim().is_empty() {
                    return Err(ContractError::InvalidBoundRule);
                }
                for value in parameters.values() {
                    value.to_rational()?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErrorComponent {
    pub rule: BoundRule,
    pub evidence: EvidenceLevel,
    pub provenance: SourceKind,
    pub citation: String,
}

impl ErrorComponent {
    pub fn is_certifying(&self) -> bool {
        !matches!(self.rule, BoundRule::None) && self.evidence.is_certifying()
    }

    pub fn exact_zero(provenance: SourceKind, citation: impl Into<String>) -> Self {
        Self {
            rule: BoundRule::Absolute {
                value: RationalSpec::integer(0),
            },
            evidence: EvidenceLevel::Analytical,
            provenance,
            citation: citation.into(),
        }
    }

    fn is_certified_zero(&self) -> bool {
        self.evidence.is_certifying()
            && matches!(
                &self.rule,
                BoundRule::Absolute { value }
                    if value.to_rational().is_ok_and(|bound| bound.is_zero())
            )
    }

    fn validate(&self) -> Result<(), ContractError> {
        self.rule.validate()?;
        if self.citation.trim().is_empty() {
            return Err(ContractError::MissingProvenanceCitation);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionRule {
    None,
    ConservativeSum,
    IntervalPropagation,
    ImplicitRelationMonotoneEnclosure,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErrorContract {
    pub quantization: ErrorComponent,
    pub approximation: ErrorComponent,
    pub verification_slack: ErrorComponent,
    pub accumulated_error: ErrorComponent,
    pub composition_rule: CompositionRule,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProfileProvenance {
    pub source_kind: SourceKind,
    pub citation: String,
    pub locator: String,
    pub note: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticClass {
    ExactReferenceSemantics,
    BoundedApproximation,
    UnboundedSemantics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionCertification {
    CertifiedStable,
    Ambiguous,
    ObservedSame,
    ObservedFlip,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DecisionBoundarySpec {
    Threshold {
        threshold: RationalSpec,
    },
    TopTwo {
        winner_index: usize,
        runner_up_index: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionMarginEvaluation {
    pub margin: BigRational,
    /// Maximum movement toward the decision boundary under the declared
    /// symmetric output bounds.
    pub boundary_error_budget: BigRational,
    pub stable: bool,
}

pub fn threshold_margin(
    value: &BigRational,
    threshold: &BigRational,
    absolute_error_bound: &BigRational,
) -> DecisionMarginEvaluation {
    let margin = (value - threshold).abs();
    DecisionMarginEvaluation {
        stable: margin > *absolute_error_bound,
        margin,
        boundary_error_budget: absolute_error_bound.clone(),
    }
}

/// Conservative argmax condition. If the two relevant logits have bounds
/// eps1 and eps2, their ordering is invariant only when top1-top2 > eps1+eps2.
/// The familiar `2*epsilon` rule is the symmetric special case.
pub fn top_two_margin(
    top1: &BigRational,
    top2: &BigRational,
    top1_absolute_error_bound: &BigRational,
    top2_absolute_error_bound: &BigRational,
) -> DecisionMarginEvaluation {
    let margin = top1 - top2;
    let budget = top1_absolute_error_bound + top2_absolute_error_bound;
    DecisionMarginEvaluation {
        stable: margin > budget,
        margin,
        boundary_error_budget: budget,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NumericContractProfile {
    pub schema_version: u16,
    pub profile_id: String,
    pub profile_version: u16,
    pub representation: RepresentationSpec,
    pub operations: OperationSemantics,
    pub valid_domain: ValidDomain,
    pub approximation: ApproximationSpec,
    pub error_contract: ErrorContract,
    pub provenance: Vec<ProfileProvenance>,
}

#[derive(Clone, Debug)]
pub struct NumericContractProfileBuilder {
    profile: NumericContractProfile,
}

impl NumericContractProfileBuilder {
    pub fn new(
        profile_id: impl Into<String>,
        representation: RepresentationSpec,
        operations: OperationSemantics,
        valid_domain: ValidDomain,
        approximation: ApproximationSpec,
        error_contract: ErrorContract,
    ) -> Self {
        Self {
            profile: NumericContractProfile {
                schema_version: NUMERIC_CONTRACT_SCHEMA_VERSION,
                profile_id: profile_id.into(),
                profile_version: 1,
                representation,
                operations,
                valid_domain,
                approximation,
                error_contract,
                provenance: Vec::new(),
            },
        }
    }

    pub fn profile_version(mut self, version: u16) -> Self {
        self.profile.profile_version = version;
        self
    }

    pub fn provenance(mut self, provenance: ProfileProvenance) -> Self {
        self.profile.provenance.push(provenance);
        self
    }

    pub fn build(self) -> Result<NumericContractProfile, ContractError> {
        self.profile.validate()?;
        Ok(self.profile)
    }
}

impl NumericContractProfile {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != NUMERIC_CONTRACT_SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema(self.schema_version));
        }
        if self.profile_id.trim().is_empty() || self.profile_version == 0 {
            return Err(ContractError::InvalidIdentity);
        }
        let (minimum, maximum) = self.valid_domain.bounds()?;
        if minimum >= maximum {
            return Err(ContractError::InvalidDomain);
        }
        if self.representation.scale_power != self.representation.fractional_bits {
            return Err(ContractError::ScaleMismatch);
        }
        if self.representation.kind == NumericRepresentationKind::SignedFixedPoint
            && (!self.representation.signed
                || self.representation.integer_bits == 0
                || self.representation.fractional_bits == 0)
        {
            return Err(ContractError::InvalidFixedPointFormat);
        }
        if self.approximation.method == ApproximationMethod::None {
            if self.approximation.order != 0 {
                return Err(ContractError::InvalidApproximationOrder);
            }
        } else if self.approximation.order == 0 {
            return Err(ContractError::InvalidApproximationOrder);
        }
        self.approximation.expansion_center.to_rational()?;
        for parameter in self.approximation.parameters.values() {
            parameter.to_rational()?;
        }
        if self.approximation.evaluation_form.trim().is_empty()
            || self
                .approximation
                .coefficient_or_node_source
                .trim()
                .is_empty()
        {
            return Err(ContractError::InvalidApproximationParameters);
        }
        self.error_contract.quantization.validate()?;
        self.error_contract.approximation.validate()?;
        self.error_contract.verification_slack.validate()?;
        self.error_contract.accumulated_error.validate()?;
        if self.semantic_class() == SemanticClass::BoundedApproximation
            && self.error_contract.composition_rule == CompositionRule::None
        {
            return Err(ContractError::MissingCompositionRule);
        }
        for provenance in &self.provenance {
            if provenance.citation.trim().is_empty() {
                return Err(ContractError::MissingProvenanceCitation);
            }
        }
        Ok(())
    }

    pub fn semantic_class(&self) -> SemanticClass {
        let contract = &self.error_contract;
        if matches!(
            self.representation.kind,
            NumericRepresentationKind::MpfrReference | NumericRepresentationKind::Binary32Reference
        ) && self.approximation.method == ApproximationMethod::None
            && contract.quantization.is_certified_zero()
            && contract.approximation.is_certified_zero()
            && contract.verification_slack.is_certified_zero()
            && contract.accumulated_error.is_certified_zero()
        {
            return SemanticClass::ExactReferenceSemantics;
        }
        if contract.quantization.is_certifying()
            && contract.approximation.is_certifying()
            && contract.verification_slack.is_certifying()
            && contract.accumulated_error.is_certifying()
            && contract.composition_rule != CompositionRule::None
        {
            SemanticClass::BoundedApproximation
        } else {
            SemanticClass::UnboundedSemantics
        }
    }

    /// Stable JSON bytes: struct field order is fixed by schema v1 and all
    /// maps are BTreeMaps. A future layout must increment schema_version.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("NumericContractProfile serializável")
    }

    pub fn profile_hash(&self) -> [u8; 32] {
        Sha256::digest(self.canonical_bytes()).into()
    }

    pub fn profile_hash_hex(&self) -> String {
        use std::fmt::Write;
        self.profile_hash()
            .iter()
            .fold(String::with_capacity(64), |mut output, byte| {
                let _ = write!(output, "{byte:02x}");
                output
            })
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, ContractError> {
        let profile: Self = serde_json::from_slice(bytes).map_err(ContractError::Json)?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn to_pretty_json(&self) -> Result<String, ContractError> {
        serde_json::to_string_pretty(self).map_err(ContractError::Json)
    }
}

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("denominador racional não pode ser zero")]
    ZeroDenominator,
    #[error("racional textual inválido")]
    InvalidRational,
    #[error("racional não está em forma canônica")]
    NonCanonicalRational,
    #[error("schema de NumericContractProfile não suportado: {0}")]
    UnsupportedSchema(u16),
    #[error("profile_id vazio ou profile_version zero")]
    InvalidIdentity,
    #[error("domínio numérico inválido")]
    InvalidDomain,
    #[error("scale_power deve coincidir com fractional_bits em fixed-point v1")]
    ScaleMismatch,
    #[error("formato fixed-point v1 inválido")]
    InvalidFixedPointFormat,
    #[error("ordem da aproximação inválida")]
    InvalidApproximationOrder,
    #[error("parâmetros da aproximação inválidos")]
    InvalidApproximationParameters,
    #[error("bound absoluto de erro não pode ser negativo")]
    NegativeErrorBound,
    #[error("regra nomeada de bound é inválida")]
    InvalidBoundRule,
    #[error("contrato bounded exige regra de composição")]
    MissingCompositionRule,
    #[error("entrada de proveniência sem citação")]
    MissingProvenanceCitation,
    #[error("JSON de perfil inválido: {0}")]
    Json(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(evidence: EvidenceLevel) -> ErrorComponent {
        ErrorComponent {
            rule: BoundRule::Named {
                rule_id: "test-rule".into(),
                parameters: BTreeMap::new(),
            },
            evidence,
            provenance: SourceKind::OurExtension,
            citation: "test".into(),
        }
    }

    fn sample(evidence: EvidenceLevel) -> NumericContractProfile {
        let representation = RepresentationSpec {
            kind: NumericRepresentationKind::SignedFixedPoint,
            signed: true,
            integer_bits: 16,
            fractional_bits: 16,
            scale_power: 16,
        };
        let operations = OperationSemantics {
            rounding: QuantizationRounding::NearestTiesToEven,
            rescaling: RescalingMode::AfterEveryMultiplication,
            division_rounding: QuantizationRounding::NearestTiesToEven,
            overflow: OverflowMode::Reject,
            underflow: UnderflowMode::QuantizeToGrid,
            exceptional_values: ExceptionalValuePolicy::RejectNonFinite,
        };
        let approximation = ApproximationSpec {
            function: FunctionId::Exp,
            method: ApproximationMethod::Taylor,
            method_version: 1,
            order: 5,
            expansion_center: RationalSpec::integer(0),
            evaluation_form: "horner-rounded-each-mul".into(),
            parameters: BTreeMap::new(),
            coefficient_or_node_source: "exact-series".into(),
        };
        let contract = ErrorContract {
            quantization: component(evidence),
            approximation: component(evidence),
            verification_slack: ErrorComponent::exact_zero(
                SourceKind::RepositoryDerived,
                "off-circuit",
            ),
            accumulated_error: component(evidence),
            composition_rule: CompositionRule::ConservativeSum,
        };
        NumericContractProfileBuilder::new(
            "q16.16-exp-taylor-5",
            representation,
            operations,
            ValidDomain::closed(
                &BigRational::from_integer((-1).into()),
                &BigRational::from_integer(1.into()),
            ),
            approximation,
            contract,
        )
        .build()
        .unwrap()
    }

    #[test]
    fn observed_components_never_claim_bounded_semantics() {
        assert_eq!(
            sample(EvidenceLevel::ObservedGrid).semantic_class(),
            SemanticClass::UnboundedSemantics
        );
        assert_eq!(
            sample(EvidenceLevel::Analytical).semantic_class(),
            SemanticClass::BoundedApproximation
        );
    }

    #[test]
    fn canonical_round_trip_and_hash_are_stable() {
        let profile = sample(EvidenceLevel::Analytical);
        let json = profile.to_pretty_json().unwrap();
        let reparsed = NumericContractProfile::from_json(json.as_bytes()).unwrap();
        assert_eq!(profile, reparsed);
        assert_eq!(profile.profile_hash_hex(), reparsed.profile_hash_hex());
        assert_eq!(profile.profile_hash_hex().len(), 64);
    }

    #[test]
    fn rational_text_must_be_canonical() {
        let noncanonical = RationalSpec {
            numerator: "2".into(),
            denominator: "4".into(),
        };
        assert!(matches!(
            noncanonical.to_rational(),
            Err(ContractError::NonCanonicalRational)
        ));
    }

    #[test]
    fn invalid_error_contracts_are_rejected() {
        let mut negative = sample(EvidenceLevel::Analytical);
        negative.error_contract.accumulated_error.rule = BoundRule::Absolute {
            value: RationalSpec::integer(-1),
        };
        assert!(matches!(
            negative.validate(),
            Err(ContractError::NegativeErrorBound)
        ));

        let mut noncanonical = sample(EvidenceLevel::Analytical);
        noncanonical.approximation.parameters.insert(
            "bad".into(),
            RationalSpec {
                numerator: "2".into(),
                denominator: "4".into(),
            },
        );
        assert!(matches!(
            noncanonical.validate(),
            Err(ContractError::NonCanonicalRational)
        ));
    }

    #[test]
    fn threshold_and_top_two_margins_document_their_assumptions() {
        let threshold = threshold_margin(
            &BigRational::new(3.into(), 4.into()),
            &BigRational::new(1.into(), 2.into()),
            &BigRational::new(1.into(), 8.into()),
        );
        assert!(threshold.stable);

        let stable = top_two_margin(
            &BigRational::new(9.into(), 10.into()),
            &BigRational::new(6.into(), 10.into()),
            &BigRational::new(1.into(), 10.into()),
            &BigRational::new(1.into(), 10.into()),
        );
        assert!(stable.stable);
        let ambiguous = top_two_margin(
            &BigRational::new(8.into(), 10.into()),
            &BigRational::new(6.into(), 10.into()),
            &BigRational::new(1.into(), 10.into()),
            &BigRational::new(1.into(), 10.into()),
        );
        assert!(!ambiguous.stable);
    }
}

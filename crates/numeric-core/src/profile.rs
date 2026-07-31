//! Identidade canônica de uma configuração experimental.
//!
//! Custo de circuito e cobertura de decisões são produzidos por binários
//! distintos. Para juntá-los sem depender de rótulos textuais divergentes,
//! ambos os lados constroem o mesmo [`ExperimentProfile`] e publicam o
//! `profile_hash_hex` definido em `docs/pareto-protocol.md`.

use num_bigint::Sign;
use num_rational::BigRational;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Versão da serialização canônica. Qualquer mudança de layout exige
/// incremento, porque ela invalida hashes gravados em rodadas anteriores.
pub const PROFILE_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkloadId {
    LogisticMap,
    StableAffine,
    AdultLogistic,
    EcefProximity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileFamily {
    FixedPoint,
    Cbd,
    Cfd,
    Fp32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundMode {
    /// Aritmética finita sem certificado de erro.
    None,
    GlobalLipschitz,
    LocalTaylor,
    /// Enclosure intervalar por extremos, usada pela CFD.
    Interval,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Topology {
    Fused,
    Sequential,
    SingleShot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoundingId {
    Truncate,
    NearestTiesToEven,
    HonestRneCertifiedEnclosure,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProfileError {
    #[error("identificador de representação vazio")]
    EmptyRepresentation,
    #[error("precision_bits deve ser positivo")]
    InvalidPrecision,
    #[error("domínio vazio: minimum deve ser menor que maximum")]
    EmptyDomain,
    #[error("nenhum limiar declarado")]
    NoThresholds,
    #[error("o limiar principal não pertence ao conjunto declarado")]
    PrincipalThresholdNotDeclared,
    #[error("parâmetro público duplicado: {0}")]
    DuplicateParameter(String),
}

/// Descreve uma configuração completa do protocolo de Pareto.
///
/// A ordem dos campos aqui é a ordem da serialização canônica; alterá-la muda
/// todos os hashes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExperimentProfile {
    schema_version: u16,
    workload: WorkloadId,
    family: ProfileFamily,
    representation: String,
    precision_bits: usize,
    bound_mode: BoundMode,
    topology: Topology,
    exponent_lanes: Vec<i16>,
    radius_bits: usize,
    rounding: RoundingId,
    parameters: Vec<(String, BigRational)>,
    thresholds: Vec<BigRational>,
    principal_threshold: BigRational,
    minimum: BigRational,
    maximum: BigRational,
}

/// Construtor incremental de [`ExperimentProfile`].
///
/// Campos opcionais assumem os valores neutros de uma família sem expoente
/// explícito nem raio: corredor vazio e `radius_bits = 0`.
#[derive(Clone, Debug)]
pub struct ExperimentProfileBuilder {
    workload: WorkloadId,
    family: ProfileFamily,
    representation: String,
    precision_bits: usize,
    bound_mode: BoundMode,
    topology: Topology,
    exponent_lanes: Vec<i16>,
    radius_bits: usize,
    rounding: RoundingId,
    parameters: Vec<(String, BigRational)>,
    thresholds: Vec<BigRational>,
    principal_threshold: Option<BigRational>,
    minimum: BigRational,
    maximum: BigRational,
}

impl ExperimentProfileBuilder {
    pub fn new(
        workload: WorkloadId,
        family: ProfileFamily,
        representation: impl Into<String>,
        precision_bits: usize,
    ) -> Self {
        Self {
            workload,
            family,
            representation: representation.into(),
            precision_bits,
            bound_mode: BoundMode::None,
            topology: Topology::Sequential,
            exponent_lanes: Vec::new(),
            radius_bits: 0,
            rounding: RoundingId::NearestTiesToEven,
            parameters: Vec::new(),
            thresholds: Vec::new(),
            principal_threshold: None,
            minimum: BigRational::from_integer(0.into()),
            maximum: BigRational::from_integer(1.into()),
        }
    }

    pub fn bound_mode(mut self, bound_mode: BoundMode) -> Self {
        self.bound_mode = bound_mode;
        self
    }

    pub fn topology(mut self, topology: Topology) -> Self {
        self.topology = topology;
        self
    }

    pub fn exponent_lanes(mut self, lanes: impl IntoIterator<Item = i16>) -> Self {
        self.exponent_lanes = lanes.into_iter().collect();
        self
    }

    pub fn radius_bits(mut self, radius_bits: usize) -> Self {
        self.radius_bits = radius_bits;
        self
    }

    pub fn rounding(mut self, rounding: RoundingId) -> Self {
        self.rounding = rounding;
        self
    }

    pub fn parameter(mut self, name: impl Into<String>, value: BigRational) -> Self {
        self.parameters.push((name.into(), value));
        self
    }

    pub fn thresholds(mut self, thresholds: impl IntoIterator<Item = BigRational>) -> Self {
        self.thresholds = thresholds.into_iter().collect();
        self
    }

    pub fn principal_threshold(mut self, threshold: BigRational) -> Self {
        self.principal_threshold = Some(threshold);
        self
    }

    pub fn domain(mut self, minimum: BigRational, maximum: BigRational) -> Self {
        self.minimum = minimum;
        self.maximum = maximum;
        self
    }

    pub fn build(self) -> Result<ExperimentProfile, ProfileError> {
        if self.representation.is_empty() {
            return Err(ProfileError::EmptyRepresentation);
        }
        if self.precision_bits == 0 {
            return Err(ProfileError::InvalidPrecision);
        }
        if self.minimum >= self.maximum {
            return Err(ProfileError::EmptyDomain);
        }

        let mut parameters = self.parameters;
        parameters.sort_by(|left, right| left.0.cmp(&right.0));
        if let Some(window) = parameters.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(ProfileError::DuplicateParameter(window[0].0.clone()));
        }

        let mut thresholds = self.thresholds;
        thresholds.sort();
        thresholds.dedup();
        if thresholds.is_empty() {
            return Err(ProfileError::NoThresholds);
        }

        let principal_threshold = self
            .principal_threshold
            .unwrap_or_else(|| thresholds[0].clone());
        if !thresholds.contains(&principal_threshold) {
            return Err(ProfileError::PrincipalThresholdNotDeclared);
        }

        let mut exponent_lanes = self.exponent_lanes;
        exponent_lanes.sort_unstable();
        exponent_lanes.dedup();

        Ok(ExperimentProfile {
            schema_version: PROFILE_SCHEMA_VERSION,
            workload: self.workload,
            family: self.family,
            representation: self.representation,
            precision_bits: self.precision_bits,
            bound_mode: self.bound_mode,
            topology: self.topology,
            exponent_lanes,
            radius_bits: self.radius_bits,
            rounding: self.rounding,
            parameters,
            thresholds,
            principal_threshold,
            minimum: self.minimum,
            maximum: self.maximum,
        })
    }
}

impl ExperimentProfile {
    pub fn builder(
        workload: WorkloadId,
        family: ProfileFamily,
        representation: impl Into<String>,
        precision_bits: usize,
    ) -> ExperimentProfileBuilder {
        ExperimentProfileBuilder::new(workload, family, representation, precision_bits)
    }

    pub fn schema_version(&self) -> u16 {
        self.schema_version
    }
    pub fn workload(&self) -> WorkloadId {
        self.workload
    }
    pub fn family(&self) -> ProfileFamily {
        self.family
    }
    pub fn representation(&self) -> &str {
        &self.representation
    }
    pub fn precision_bits(&self) -> usize {
        self.precision_bits
    }
    pub fn bound_mode(&self) -> BoundMode {
        self.bound_mode
    }
    pub fn topology(&self) -> Topology {
        self.topology
    }
    pub fn exponent_lanes(&self) -> &[i16] {
        &self.exponent_lanes
    }
    pub fn radius_bits(&self) -> usize {
        self.radius_bits
    }
    pub fn rounding(&self) -> RoundingId {
        self.rounding
    }
    pub fn parameters(&self) -> &[(String, BigRational)] {
        &self.parameters
    }
    pub fn thresholds(&self) -> &[BigRational] {
        &self.thresholds
    }
    pub fn principal_threshold(&self) -> &BigRational {
        &self.principal_threshold
    }
    pub fn minimum(&self) -> &BigRational {
        &self.minimum
    }
    pub fn maximum(&self) -> &BigRational {
        &self.maximum
    }

    pub fn parameter(&self, name: &str) -> Option<&BigRational> {
        self.parameters
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// Serialização independente de JSON usada pelo compromisso de perfil.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.schema_version.to_be_bytes());
        put_bytes(&mut out, workload_tag(self.workload).as_bytes());
        put_bytes(&mut out, family_tag(self.family).as_bytes());
        put_bytes(&mut out, self.representation.as_bytes());
        out.extend_from_slice(&(self.precision_bits as u32).to_be_bytes());
        put_bytes(&mut out, bound_mode_tag(self.bound_mode).as_bytes());
        put_bytes(&mut out, topology_tag(self.topology).as_bytes());
        out.extend_from_slice(&(self.exponent_lanes.len() as u32).to_be_bytes());
        for lane in &self.exponent_lanes {
            out.extend_from_slice(&lane.to_be_bytes());
        }
        out.extend_from_slice(&(self.radius_bits as u32).to_be_bytes());
        put_bytes(&mut out, rounding_tag(self.rounding).as_bytes());
        out.extend_from_slice(&(self.parameters.len() as u32).to_be_bytes());
        for (name, value) in &self.parameters {
            put_bytes(&mut out, name.as_bytes());
            put_rational(&mut out, value);
        }
        out.extend_from_slice(&(self.thresholds.len() as u32).to_be_bytes());
        for threshold in &self.thresholds {
            put_rational(&mut out, threshold);
        }
        put_rational(&mut out, &self.principal_threshold);
        put_rational(&mut out, &self.minimum);
        put_rational(&mut out, &self.maximum);
        out
    }

    pub fn profile_hash(&self) -> [u8; 32] {
        Sha256::digest(self.canonical_bytes()).into()
    }

    pub fn profile_hash_hex(&self) -> String {
        use std::fmt::Write;
        self.profile_hash().iter().fold(
            String::with_capacity(64),
            |mut accumulator, byte| {
                let _ = write!(accumulator, "{byte:02x}");
                accumulator
            },
        )
    }
}

fn workload_tag(value: WorkloadId) -> &'static str {
    match value {
        WorkloadId::LogisticMap => "logistic-map",
        WorkloadId::StableAffine => "stable-affine",
        WorkloadId::AdultLogistic => "adult-logistic",
        WorkloadId::EcefProximity => "ecef-proximity",
    }
}

fn family_tag(value: ProfileFamily) -> &'static str {
    match value {
        ProfileFamily::FixedPoint => "fixed-point",
        ProfileFamily::Cbd => "cbd",
        ProfileFamily::Cfd => "cfd",
        ProfileFamily::Fp32 => "fp32",
    }
}

fn bound_mode_tag(value: BoundMode) -> &'static str {
    match value {
        BoundMode::None => "none",
        BoundMode::GlobalLipschitz => "global-lipschitz",
        BoundMode::LocalTaylor => "local-taylor",
        BoundMode::Interval => "interval",
    }
}

fn topology_tag(value: Topology) -> &'static str {
    match value {
        Topology::Fused => "fused",
        Topology::Sequential => "sequential",
        Topology::SingleShot => "single-shot",
    }
}

fn rounding_tag(value: RoundingId) -> &'static str {
    match value {
        RoundingId::Truncate => "truncate",
        RoundingId::NearestTiesToEven => "nearest-ties-to-even",
        RoundingId::HonestRneCertifiedEnclosure => "honest-rne-certified-enclosure",
    }
}

impl WorkloadId {
    pub fn tag(self) -> &'static str {
        workload_tag(self)
    }
}

impl ProfileFamily {
    pub fn tag(self) -> &'static str {
        family_tag(self)
    }
}

impl BoundMode {
    pub fn tag(self) -> &'static str {
        bound_mode_tag(self)
    }
}

impl Topology {
    pub fn tag(self) -> &'static str {
        topology_tag(self)
    }
}

impl RoundingId {
    pub fn tag(self) -> &'static str {
        rounding_tag(self)
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

#[cfg(test)]
mod tests {
    use super::*;
    use num_traits::One;

    fn sample() -> ExperimentProfileBuilder {
        ExperimentProfile::builder(
            WorkloadId::LogisticMap,
            ProfileFamily::Cbd,
            "cbd-local-taylor-32",
            32,
        )
        .bound_mode(BoundMode::LocalTaylor)
        .topology(Topology::Fused)
        .parameter("r", BigRational::new(37.into(), 10.into()))
        .parameter("x0", BigRational::new(1.into(), 10.into()))
        .thresholds([
            BigRational::new(1.into(), 2.into()),
            BigRational::new(1.into(), 4.into()),
        ])
        .principal_threshold(BigRational::new(1.into(), 2.into()))
    }

    #[test]
    fn hash_is_stable_and_hex_encoded() {
        let profile = sample().build().unwrap();
        let hex = profile.profile_hash_hex();
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
        assert_eq!(hex, sample().build().unwrap().profile_hash_hex());
    }

    #[test]
    fn parameter_and_threshold_order_do_not_change_the_hash() {
        let direct = sample().build().unwrap();
        let permuted = ExperimentProfile::builder(
            WorkloadId::LogisticMap,
            ProfileFamily::Cbd,
            "cbd-local-taylor-32",
            32,
        )
        .bound_mode(BoundMode::LocalTaylor)
        .topology(Topology::Fused)
        .parameter("x0", BigRational::new(1.into(), 10.into()))
        .parameter("r", BigRational::new(37.into(), 10.into()))
        .thresholds([
            BigRational::new(1.into(), 4.into()),
            BigRational::new(1.into(), 2.into()),
            BigRational::new(1.into(), 4.into()),
        ])
        .principal_threshold(BigRational::new(1.into(), 2.into()))
        .build()
        .unwrap();
        assert_eq!(direct.profile_hash_hex(), permuted.profile_hash_hex());
    }

    #[test]
    fn distinct_configurations_produce_distinct_hashes() {
        let base = sample().build().unwrap();
        let other_bound = sample().bound_mode(BoundMode::GlobalLipschitz).build().unwrap();
        let other_topology = sample().topology(Topology::Sequential).build().unwrap();
        let other_parameter = ExperimentProfile::builder(
            WorkloadId::LogisticMap,
            ProfileFamily::Cbd,
            "cbd-local-taylor-32",
            32,
        )
        .bound_mode(BoundMode::LocalTaylor)
        .topology(Topology::Fused)
        .parameter("r", BigRational::new(39.into(), 10.into()))
        .parameter("x0", BigRational::new(1.into(), 10.into()))
        .thresholds([BigRational::new(1.into(), 2.into())])
        .build()
        .unwrap();
        for other in [other_bound, other_topology, other_parameter] {
            assert_ne!(base.profile_hash_hex(), other.profile_hash_hex());
        }
    }

    #[test]
    fn principal_threshold_must_belong_to_the_declared_set() {
        let error = sample()
            .principal_threshold(BigRational::one())
            .build()
            .unwrap_err();
        assert_eq!(error, ProfileError::PrincipalThresholdNotDeclared);
    }

    #[test]
    fn duplicate_parameters_are_rejected() {
        let error = sample()
            .parameter("r", BigRational::new(4.into(), 1.into()))
            .build()
            .unwrap_err();
        assert_eq!(error, ProfileError::DuplicateParameter("r".to_string()));
    }
}

//! Catálogo de representações da fronteira de Pareto.
//!
//! O custo de circuito e a cobertura de decisões são medidos por binários
//! diferentes. Se cada um construísse o seu [`ExperimentProfile`], uma
//! divergência de um único campo produziria dois `profile_hash_hex` para a
//! mesma configuração e a junção falharia em silêncio, devolvendo uma fronteira
//! vazia em vez de um erro. Por isso a construção do perfil mora aqui, e os
//! dois lados chamam [`FrontierPoint::profile`].

use crate::profile::{
    BoundMode, ExperimentProfile, ProfileError, ProfileFamily, RoundingId, Topology, WorkloadId,
};
use crate::{CommitmentPolicy, PrecisionPolicy};
use num_rational::BigRational;
use serde::{Deserialize, Serialize};

/// Representação numérica, identificada pelo rótulo já usado nos resultados
/// gravados desde a rodada 0007.
///
/// O rótulo não distingue workload: `cfd_ieee_shaped_sequential` descreve o
/// mesmo formato numérico no mapa logístico e na recorrência afim. Quem separa
/// as duas execuções é o campo de workload do perfil. Sem isso, a análise de
/// dominância por regime não conseguiria seguir uma representação entre
/// workloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepresentationId {
    Standard,
    Carry,
    Compensated,
    CbdLipschitz,
    CbdLocalTaylor,
    CbdSequentialLocalTaylor,
    CdadFixed,
    CdadSequential,
    CdadMarginDirected,
    CfdSequential,
    CfdFused,
    Fp32Exact,
}

impl RepresentationId {
    pub const ALL: [Self; 12] = [
        Self::Standard,
        Self::Carry,
        Self::Compensated,
        Self::CbdLipschitz,
        Self::CbdLocalTaylor,
        Self::CbdSequentialLocalTaylor,
        Self::CdadFixed,
        Self::CdadSequential,
        Self::CdadMarginDirected,
        Self::CfdSequential,
        Self::CfdFused,
        Self::Fp32Exact,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Carry => "carry",
            Self::Compensated => "compensated",
            Self::CbdLipschitz => "cbd_lipschitz",
            Self::CbdLocalTaylor => "cbd_local_taylor",
            Self::CbdSequentialLocalTaylor => "cbd_sequential_local_taylor",
            Self::CdadFixed => "cdad_fixed",
            Self::CdadSequential => "cdad_sequential",
            Self::CdadMarginDirected => "cdad_margin_directed",
            Self::CfdSequential => "cfd_ieee_shaped_sequential",
            Self::CfdFused => "cfd_ieee_shaped_fused_checkpoint",
            Self::Fp32Exact => "fp32_positive_normal_exact",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.tag() == tag)
    }

    pub fn family(self) -> ProfileFamily {
        match self {
            Self::Standard | Self::Carry | Self::Compensated => ProfileFamily::FixedPoint,
            Self::CbdLipschitz | Self::CbdLocalTaylor | Self::CbdSequentialLocalTaylor => {
                ProfileFamily::Cbd
            }
            Self::CdadFixed | Self::CdadSequential | Self::CdadMarginDirected => {
                ProfileFamily::Cdad
            }
            Self::CfdSequential | Self::CfdFused => ProfileFamily::Cfd,
            Self::Fp32Exact => ProfileFamily::Fp32,
        }
    }

    pub fn bound_mode(self) -> BoundMode {
        match self {
            Self::Standard | Self::Carry | Self::Compensated | Self::Fp32Exact => BoundMode::None,
            Self::CbdLipschitz => BoundMode::GlobalLipschitz,
            Self::CbdLocalTaylor | Self::CbdSequentialLocalTaylor => BoundMode::LocalTaylor,
            Self::CdadFixed | Self::CdadSequential | Self::CdadMarginDirected => {
                BoundMode::DecisionAdaptive
            }
            Self::CfdSequential | Self::CfdFused => BoundMode::Interval,
        }
    }

    pub fn topology(self) -> Topology {
        match self {
            Self::CbdLipschitz | Self::CbdLocalTaylor | Self::CfdFused => Topology::Fused,
            _ => Topology::Sequential,
        }
    }

    /// Arredondamento fixado pela representação.
    ///
    /// `None` significa que a representação aceita mais de um: só o ponto fixo
    /// está nesse caso, porque truncamento e par mais próximo produzem
    /// trajetórias distintas e ambos são medidos.
    pub fn rounding(self) -> Option<RoundingId> {
        match self {
            Self::Standard | Self::Carry | Self::Compensated => None,
            Self::CbdLipschitz | Self::CbdLocalTaylor | Self::CbdSequentialLocalTaylor => {
                Some(RoundingId::NearestTiesToEven)
            }
            Self::CdadFixed | Self::CdadSequential | Self::CdadMarginDirected => {
                Some(RoundingId::NearestTiesToEven)
            }
            Self::CfdSequential | Self::CfdFused => Some(RoundingId::HonestRneCertifiedEnclosure),
            Self::Fp32Exact => Some(RoundingId::NearestTiesToEven),
        }
    }

    /// Se a representação certifica um enclosure e portanto pode abster-se.
    pub fn is_certified(self) -> bool {
        matches!(
            self.bound_mode(),
            BoundMode::GlobalLipschitz
                | BoundMode::LocalTaylor
                | BoundMode::DecisionAdaptive
                | BoundMode::Interval
        )
    }

    pub fn precision_policy(self) -> Option<PrecisionPolicy> {
        match self {
            Self::CdadFixed => Some(PrecisionPolicy::Fixed),
            Self::CdadSequential => Some(PrecisionPolicy::Sequential),
            Self::CdadMarginDirected => Some(PrecisionPolicy::MarginDirected),
            _ => None,
        }
    }
}

/// Parte do perfil que depende do problema, não da representação.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkloadSpec {
    workload: WorkloadId,
    minimum: BigRational,
    maximum: BigRational,
    parameters: Vec<(String, BigRational)>,
}

impl WorkloadSpec {
    pub fn new(
        workload: WorkloadId,
        minimum: BigRational,
        maximum: BigRational,
        parameters: impl IntoIterator<Item = (String, BigRational)>,
    ) -> Self {
        Self {
            workload,
            minimum,
            maximum,
            parameters: parameters.into_iter().collect(),
        }
    }

    pub fn logistic_map(r: BigRational, x0: BigRational) -> Self {
        Self::new(
            WorkloadId::LogisticMap,
            BigRational::from_integer(0.into()),
            BigRational::from_integer(1.into()),
            [("r".to_string(), r), ("x0".to_string(), x0)],
        )
    }

    pub fn stable_affine(alpha: BigRational, innovation: BigRational, x0: BigRational) -> Self {
        Self::new(
            WorkloadId::StableAffine,
            BigRational::from_integer(0.into()),
            BigRational::from_integer(2.into()),
            [
                ("alpha".to_string(), alpha),
                ("innovation".to_string(), innovation),
                ("x0".to_string(), x0),
            ],
        )
    }

    /// Domínio assinado compartilhado por Adult e ECEF.
    ///
    /// Os dois workloads acumulam somas de produtos que saem de `[0,1]`, e a
    /// avaliação CBD desde 0008 usa `[-64,64]`. Mantê-lo aqui evita que uma
    /// representação nova estreite o domínio e ganhe cobertura por isso.
    pub fn signed_scoring(
        workload: WorkloadId,
        parameters: impl IntoIterator<Item = (String, BigRational)>,
    ) -> Self {
        Self::new(
            workload,
            BigRational::from_integer((-64).into()),
            BigRational::from_integer(64.into()),
            parameters,
        )
    }

    pub fn workload(&self) -> WorkloadId {
        self.workload
    }
    pub fn minimum(&self) -> &BigRational {
        &self.minimum
    }
    pub fn maximum(&self) -> &BigRational {
        &self.maximum
    }
    pub fn parameters(&self) -> &[(String, BigRational)] {
        &self.parameters
    }
}

/// Uma configuração completa: um ponto da fronteira.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontierPoint {
    pub representation: RepresentationId,
    pub spec: WorkloadSpec,
    pub precision_bits: usize,
    pub exponent_lanes: Vec<i16>,
    pub radius_bits: usize,
    pub rounding: RoundingId,
    pub threshold: BigRational,
}

impl FrontierPoint {
    /// Ponto sem corredor de expoentes, para ponto fixo, CBD e FP32.
    pub fn plain(
        representation: RepresentationId,
        spec: WorkloadSpec,
        precision_bits: usize,
        rounding: RoundingId,
        threshold: BigRational,
    ) -> Self {
        Self {
            representation,
            spec,
            precision_bits,
            exponent_lanes: Vec::new(),
            radius_bits: 0,
            rounding,
            threshold,
        }
    }

    pub fn profile(&self) -> Result<ExperimentProfile, ProfileError> {
        let mut builder = ExperimentProfile::builder(
            self.spec.workload(),
            self.representation.family(),
            self.representation.tag(),
            self.precision_bits,
        )
        .bound_mode(self.representation.bound_mode())
        .topology(self.representation.topology())
        .rounding(self.rounding)
        .thresholds([self.threshold.clone()])
        .principal_threshold(self.threshold.clone())
        .domain(self.spec.minimum().clone(), self.spec.maximum().clone());
        for (name, value) in self.spec.parameters() {
            builder = builder.parameter(name.clone(), value.clone());
        }
        // Corredor e raio só existem na CFD. Emiti-los em outras famílias
        // faria o hash depender de um campo que a representação ignora.
        if self.representation.family() == ProfileFamily::Cfd {
            builder = builder
                .exponent_lanes(self.exponent_lanes.iter().copied())
                .radius_bits(self.radius_bits);
        }
        if self.representation.family() == ProfileFamily::Cdad {
            builder = builder.adaptive(
                [16, 32, 48, 64],
                self.representation
                    .precision_policy()
                    .expect("representação CDAD possui política"),
                CommitmentPolicy::ExistingPoseidonLink,
            );
        }
        builder.build()
    }

    pub fn profile_hash_hex(&self) -> Result<String, ProfileError> {
        Ok(self.profile()?.profile_hash_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logistic() -> WorkloadSpec {
        WorkloadSpec::logistic_map(
            BigRational::new(37.into(), 10.into()),
            BigRational::new(1.into(), 10.into()),
        )
    }

    fn half() -> BigRational {
        BigRational::new(1.into(), 2.into())
    }

    /// Hashes conferidos uma vez executando `nova-circuit` e `frontier-eval` e
    /// comparando as duas saídas na rodada 0019.
    ///
    /// Fixá-los aqui transforma qualquer mudança no esquema do perfil em um teste
    /// vermelho. Sem isso a mudança seria silenciosa: os dois lados continuariam
    /// concordando entre si, com hashes novos, e os resultados de custo já
    /// gravados deixariam de juntar-se aos de cobertura sem nenhum erro visível.
    /// Se este teste falhar, a correção não é atualizar a constante — é decidir se
    /// os resultados anteriores ainda são comparáveis e regravá-los se não forem.
    #[test]
    fn hashes_are_stable_across_runs() {
        let golden = [
            (
                RepresentationId::Standard,
                32,
                RoundingId::NearestTiesToEven,
                "b7077c283939eb18c4ea1896d1fc4e9433329039673c318235a7c8b36dfcf64c",
            ),
            (
                RepresentationId::Carry,
                32,
                RoundingId::NearestTiesToEven,
                "337558e57fd241f47d2efa2d98f530d7f417e9a0a98e8cb3cfad236c8fbdec3d",
            ),
            (
                RepresentationId::Compensated,
                32,
                RoundingId::NearestTiesToEven,
                "bb649c9de5c9672bbc6bf2a7594f5a5d4e92900cfeab29ee71b051e99023edb4",
            ),
            (
                RepresentationId::CbdLipschitz,
                32,
                RoundingId::NearestTiesToEven,
                "940571474d02305688390a28023a373563dcbe871ae76bdeacd36809a3e121f9",
            ),
            (
                RepresentationId::CbdLocalTaylor,
                32,
                RoundingId::NearestTiesToEven,
                "264d34141faf454486fc7df16bb543833f7a1f341a4dd7c4e7617d802f8826a6",
            ),
            (
                RepresentationId::CbdSequentialLocalTaylor,
                32,
                RoundingId::NearestTiesToEven,
                "7d94880a25476fd127bb3a7587d07577790321182f7d82c461899e364db7350c",
            ),
            (
                RepresentationId::Fp32Exact,
                24,
                RoundingId::NearestTiesToEven,
                "2ba3d32b43eb71cc66fe07159e73760ed8f98838a2d8f0666cb55981775fdd42",
            ),
        ];
        for (representation, precision_bits, rounding, expected) in golden {
            let point =
                FrontierPoint::plain(representation, logistic(), precision_bits, rounding, half());
            assert_eq!(
                point.profile_hash_hex().unwrap(),
                expected,
                "hash de {} mudou",
                representation.tag()
            );
        }

        let cfd = FrontierPoint {
            representation: RepresentationId::CfdSequential,
            spec: logistic(),
            precision_bits: 24,
            exponent_lanes: (-8..=0).collect(),
            radius_bits: 16,
            rounding: RoundingId::HonestRneCertifiedEnclosure,
            threshold: half(),
        };
        assert_eq!(
            cfd.profile_hash_hex().unwrap(),
            "776550dd98cd3dbeceeea0518985644d5789318aa0ba3c633ad85ec491bfe1e4"
        );
        let fused = FrontierPoint {
            representation: RepresentationId::CfdFused,
            ..cfd
        };
        assert_eq!(
            fused.profile_hash_hex().unwrap(),
            "27fe47e08b609369be24106e2b1599ac3935519a7bb9d1f9db4f5318605191d1"
        );
    }

    #[test]
    fn every_representation_has_a_unique_tag() {
        let mut tags: Vec<&str> = RepresentationId::ALL.iter().map(|r| r.tag()).collect();
        tags.sort_unstable();
        let total = tags.len();
        tags.dedup();
        assert_eq!(tags.len(), total);
    }

    #[test]
    fn tags_round_trip() {
        for representation in RepresentationId::ALL {
            assert_eq!(
                RepresentationId::from_tag(representation.tag()),
                Some(representation)
            );
        }
    }

    #[test]
    fn distinct_representations_produce_distinct_hashes() {
        let mut hashes: Vec<String> = RepresentationId::ALL
            .into_iter()
            .map(|representation| {
                FrontierPoint::plain(
                    representation,
                    logistic(),
                    32,
                    representation
                        .rounding()
                        .unwrap_or(RoundingId::NearestTiesToEven),
                    half(),
                )
                .profile_hash_hex()
                .unwrap()
            })
            .collect();
        let total = hashes.len();
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), total);
    }

    #[test]
    fn exponent_corridor_is_ignored_outside_cfd() {
        let base = FrontierPoint::plain(
            RepresentationId::CbdLocalTaylor,
            logistic(),
            32,
            RoundingId::NearestTiesToEven,
            half(),
        );
        let mut noisy = base.clone();
        noisy.exponent_lanes = (-8..=0).collect();
        noisy.radius_bits = 16;
        assert_eq!(
            base.profile_hash_hex().unwrap(),
            noisy.profile_hash_hex().unwrap()
        );
    }

    #[test]
    fn exponent_corridor_changes_the_cfd_hash() {
        let narrow = FrontierPoint {
            representation: RepresentationId::CfdSequential,
            spec: logistic(),
            precision_bits: 24,
            exponent_lanes: (-4..=0).collect(),
            radius_bits: 16,
            rounding: RoundingId::HonestRneCertifiedEnclosure,
            threshold: half(),
        };
        let mut wide = narrow.clone();
        wide.exponent_lanes = (-8..=0).collect();
        assert_ne!(
            narrow.profile_hash_hex().unwrap(),
            wide.profile_hash_hex().unwrap()
        );
    }

    #[test]
    fn the_same_representation_keeps_its_tag_across_workloads() {
        let logistic_point = FrontierPoint::plain(
            RepresentationId::CfdSequential,
            logistic(),
            24,
            RoundingId::HonestRneCertifiedEnclosure,
            half(),
        );
        let stable_point = FrontierPoint::plain(
            RepresentationId::CfdSequential,
            WorkloadSpec::stable_affine(
                BigRational::new(1.into(), 2.into()),
                BigRational::new(3.into(), 4.into()),
                BigRational::new(1.into(), 10.into()),
            ),
            24,
            RoundingId::HonestRneCertifiedEnclosure,
            BigRational::from_integer(1.into()),
        );
        assert_eq!(
            logistic_point.profile().unwrap().representation(),
            stable_point.profile().unwrap().representation()
        );
        assert_ne!(
            logistic_point.profile_hash_hex().unwrap(),
            stable_point.profile_hash_hex().unwrap()
        );
    }
}

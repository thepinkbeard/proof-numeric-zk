//! Reusable off-circuit approximations with explicit operation traces.

mod gauss_legendre;
mod polynomial;
mod series;
mod certified_sigmoid;
pub use certified_sigmoid::{prepare_sigmoid_contract, generate_sigmoid_certificate};

use num_rational::BigRational;
use numeric_core::{FixedPointError, FixedPointFormat};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use gauss_legendre::{
    evaluate_gauss_legendre, evaluate_gauss_legendre_prepared, gauss_legendre_relation,
    prepare_gauss_legendre, GaussLegendreEvaluation, PreparedGaussLegendre,
};
pub use polynomial::{evaluate_pade, evaluate_taylor, PolynomialEvaluation};
pub use series::{pade_diagonal_coefficients, taylor_coefficients};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationCounts {
    pub additions: u64,
    pub multiplications: u64,
    pub divisions: u64,
    pub inversions: u64,
    pub terms: u64,
    pub numerical_multiplicative_depth: u32,
    pub verification_relation_depth: Option<u32>,
    pub conceptual_witness_bytes: u64,
}

impl OperationCounts {
    pub fn total_operations(&self) -> u64 {
        self.additions + self.multiplications + self.divisions + self.inversions
    }

    pub fn add_assign(&mut self, other: &Self) {
        self.additions += other.additions;
        self.multiplications += other.multiplications;
        self.divisions += other.divisions;
        self.inversions += other.inversions;
        self.terms += other.terms;
        self.numerical_multiplicative_depth = self
            .numerical_multiplicative_depth
            .max(other.numerical_multiplicative_depth);
        self.conceptual_witness_bytes += other.conceptual_witness_bytes;
    }
}

pub fn conceptual_value_bytes(format: &FixedPointFormat) -> u64 {
    let total_bits = 1_u64 + u64::from(format.integer_bits) + u64::from(format.fractional_bits);
    total_bits.div_ceil(8)
}

pub fn polynomial_exact(coefficients: &[BigRational], input: &BigRational) -> BigRational {
    coefficients.iter().rev().fold(
        BigRational::from_integer(0.into()),
        |accumulator, coefficient| accumulator * input + coefficient,
    )
}

#[derive(Clone, Debug, Error)]
pub enum ApproximationError {
    #[error("função não suportada pelo método selecionado")]
    UnsupportedFunction,
    #[error("ordem da aproximação deve ser positiva")]
    InvalidOrder,
    #[error("sistema linear singular ao construir Padé")]
    SingularPadeSystem,
    #[error("denominador de Padé é zero")]
    ZeroPadeDenominator,
    #[error("relação Gauss-Legendre não contém uma raiz no domínio público")]
    UnbracketedGaussLegendreRoot,
    #[error("candidato Gauss-Legendre não cabe no formato fixed-point")]
    NoFixedPointCandidate,
    #[error("regra Gauss-Legendre inválida: {0}")]
    InvalidGaussLegendreRule(String),
    #[error(transparent)]
    FixedPoint(#[from] FixedPointError),
}

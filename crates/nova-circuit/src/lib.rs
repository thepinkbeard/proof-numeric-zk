//! Relação R1CS de uma etapa do mapa logístico em limbs.
//!
//! A primeira versão implementa `P=16`, com `K<=4`. Todas as divisões inteiras
//! são verificadas como igualdades com resto e range checks; truncagem e
//! arredondamento para o mais próximo com empate para par são constrained.

use ff::PrimeField;
use nova_snark::{
    frontend::{
        num::AllocatedNum, AllocatedBit, ConstraintSystem, LinearCombination, SynthesisError,
    },
    traits::circuit::StepCircuit,
};
use num_bigint::BigUint;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive};

mod affine;
pub mod escalation;
mod cfd;
mod fp32;

pub use affine::{
    AffineParameters, AffineStepCircuit, AffineStepWitness, CbdAffineStepCircuit,
    CbdAffineStepWitness,
};

pub use cfd::{
    circuit_decision, common_threshold, CfdCircuitValue, CfdLogisticStepCircuit,
    CfdLogisticStepWitness, CfdStableStepCircuit, CfdStableStepWitness, CfdTopology,
};

pub use fp32::{
    power_of_two_biased_exponent, CircuitFp32, Fp32AddCircuit, Fp32AddWitness, Fp32MulCircuit,
    Fp32MulWitness, Fp32SubOneCircuit, Fp32SubOneWitness, RestrictedFp32StableStepCircuit,
    RestrictedFp32StableStepWitness, RestrictedFp32StepCircuit, RestrictedFp32StepWitness,
    HALF_BIASED_EXPONENT, ONE_BIASED_EXPONENT,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepWitness<const K: usize> {
    pub input_limbs: [u64; K],
    pub input_is_one: bool,
    pub product_floor: u128,
    pub quotient_product: u128,
    pub remainder_product: u128,
    pub product_rounded_up: bool,
    pub scale_floor: u128,
    pub output_limbs: [u64; K],
    pub output_is_one: bool,
    pub remainder_scale: u64,
    pub scale_rounded_up: bool,
}

impl<const K: usize> StepWitness<K> {
    pub fn compute(input_limbs: [u64; K], input_is_one: bool, r_num: u64, r_den: u64) -> Self {
        Self::compute_with_rounding(
            input_limbs,
            input_is_one,
            r_num,
            r_den,
            CircuitRounding::Truncate,
        )
    }

    pub fn compute_with_rounding(
        input_limbs: [u64; K],
        input_is_one: bool,
        r_num: u64,
        r_den: u64,
        rounding: CircuitRounding,
    ) -> Self {
        assert!(K > 0 && K <= 4);
        assert!(r_den > 0);
        let scale = 1_u128 << (16 * K);
        let input = if input_is_one {
            scale
        } else {
            input_limbs
                .iter()
                .fold(0_u128, |acc, &limb| (acc << 16) + limb as u128)
        };
        let raw_product = input * (scale - input);
        let product_floor = raw_product / scale;
        let remainder_product = raw_product % scale;
        let product_rounded_up =
            should_round_up_u128(product_floor, remainder_product, scale, rounding);
        let quotient_product = product_floor + u128::from(product_rounded_up);
        let scaled = quotient_product * r_num as u128;
        let scale_floor = scaled / r_den as u128;
        let remainder_scale = (scaled % r_den as u128) as u64;
        let scale_rounded_up = should_round_up_u128(
            scale_floor,
            remainder_scale as u128,
            r_den as u128,
            rounding,
        );
        let output = scale_floor + u128::from(scale_rounded_up);
        assert!(output <= scale, "mapa logístico saiu de [0,1]");
        let output_is_one = output == scale;
        let mut remaining = if output_is_one { 0 } else { output };
        let mut output_limbs = [0_u64; K];
        for limb in output_limbs.iter_mut().rev() {
            *limb = (remaining & 0xffff) as u64;
            remaining >>= 16;
        }
        Self {
            input_limbs,
            input_is_one,
            product_floor,
            quotient_product,
            remainder_product,
            product_rounded_up,
            scale_floor,
            output_limbs,
            output_is_one,
            remainder_scale,
            scale_rounded_up,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CircuitRounding {
    Truncate,
    NearestTiesToEven,
}

impl CircuitRounding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Truncate => "truncate",
            Self::NearestTiesToEven => "nearest_ties_to_even",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogisticStepCircuit<const K: usize> {
    pub witness: StepWitness<K>,
    pub r_num: u64,
    pub r_den: u64,
    pub rounding: CircuitRounding,
    /// Limiar público do predicado, no numerador da escala `2^(16K)`.
    pub threshold_numerator: u128,
}

/// Testemunha para uma etapa CBD fusionada em `[0,1]`.
///
/// O centro verifica `round(r*N*(S-N)/(D*S))`. O raio usa o bound de
/// Lipschitz `ceil(r*R/D)` mais uma ULP quando a quantização do centro é
/// inexata, saturando em `S` (o domínio inteiro).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CbdStepWitness<const K: usize> {
    pub input_limbs: [u64; K],
    pub input_is_one: bool,
    pub input_radius: u128,
    pub center_floor: u128,
    pub center_remainder: u128,
    pub center_rounded_up: bool,
    pub output_limbs: [u64; K],
    pub output_is_one: bool,
    pub center_above_half: bool,
    pub center_distance: u128,
    pub radius_floor: u128,
    pub radius_remainder: u128,
    pub uncapped_radius: u128,
    pub output_radius: u128,
    pub radius_capped: bool,
    /// 0=false, 1=true, 2=indeterminate para o predicado `x>=t`, onde `t` é o
    /// limiar público do perfil expresso no numerador da escala `2^(16K)`.
    pub decision_half: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CbdRadiusBound {
    Global,
    LocalTaylor,
}

/// Numerador do limiar `1/2` na escala de `K` limbs de 16 bits.
pub const fn half_threshold_numerator(k: usize) -> u128 {
    (1_u128 << (16 * k)) / 2
}

impl<const K: usize> CbdStepWitness<K> {
    pub fn compute(
        input_limbs: [u64; K],
        input_is_one: bool,
        input_radius: u128,
        r_num: u64,
        r_den: u64,
    ) -> Self {
        Self::compute_with_bound(
            input_limbs,
            input_is_one,
            input_radius,
            r_num,
            r_den,
            CbdRadiusBound::Global,
            half_threshold_numerator(K),
        )
    }

    pub fn compute_local(
        input_limbs: [u64; K],
        input_is_one: bool,
        input_radius: u128,
        r_num: u64,
        r_den: u64,
    ) -> Self {
        Self::compute_with_bound(
            input_limbs,
            input_is_one,
            input_radius,
            r_num,
            r_den,
            CbdRadiusBound::LocalTaylor,
            half_threshold_numerator(K),
        )
    }

    /// Variante do protocolo de Pareto: o limiar é um parâmetro público.
    pub fn compute_with_threshold(
        input_limbs: [u64; K],
        input_is_one: bool,
        input_radius: u128,
        r_num: u64,
        r_den: u64,
        radius_bound: CbdRadiusBound,
        threshold_numerator: u128,
    ) -> Self {
        Self::compute_with_bound(
            input_limbs,
            input_is_one,
            input_radius,
            r_num,
            r_den,
            radius_bound,
            threshold_numerator,
        )
    }

    fn compute_with_bound(
        input_limbs: [u64; K],
        input_is_one: bool,
        input_radius: u128,
        r_num: u64,
        r_den: u64,
        radius_bound: CbdRadiusBound,
        threshold_numerator: u128,
    ) -> Self {
        assert!(K > 0 && K <= 4);
        assert!(r_den > 0);
        let scale = 1_u128 << (16 * K);
        assert!(input_radius <= scale);
        let input = if input_is_one {
            scale
        } else {
            input_limbs
                .iter()
                .fold(0_u128, |acc, &limb| (acc << 16) + limb as u128)
        };

        let raw = BigUint::from(input) * BigUint::from(scale - input) * BigUint::from(r_num);
        let divisor = BigUint::from(r_den) * BigUint::from(scale);
        let center_floor = (&raw / &divisor)
            .to_u128()
            .expect("centro CBD cabe em u128");
        let center_remainder = (&raw % &divisor).to_u128().expect("resto CBD cabe em u128");
        let divisor_u128 = divisor.to_u128().expect("divisor CBD cabe em u128");
        let center_rounded_up = should_round_up_u128(
            center_floor,
            center_remainder,
            divisor_u128,
            CircuitRounding::NearestTiesToEven,
        );
        let output = center_floor + u128::from(center_rounded_up);
        assert!(output <= scale);
        let output_is_one = output == scale;
        let mut remaining = if output_is_one { 0 } else { output };
        let mut output_limbs = [0_u64; K];
        for limb in output_limbs.iter_mut().rev() {
            *limb = (remaining & 0xffff) as u64;
            remaining >>= 16;
        }

        let center_above_half = input.saturating_mul(2) >= scale;
        let center_distance = if center_above_half {
            input * 2 - scale
        } else {
            scale - input * 2
        };
        let (radius_floor, radius_remainder) = match radius_bound {
            CbdRadiusBound::Global => {
                let radius_numerator = input_radius * r_num as u128;
                (
                    radius_numerator / r_den as u128,
                    radius_numerator % r_den as u128,
                )
            }
            CbdRadiusBound::LocalTaylor => {
                let radius = BigUint::from(input_radius);
                let error_term = &radius * BigUint::from(center_distance) + &radius * &radius;
                let numerator = error_term * BigUint::from(r_num);
                let denominator = BigUint::from(r_den) * BigUint::from(scale);
                (
                    (&numerator / &denominator)
                        .to_u128()
                        .expect("bound local cabe em u128"),
                    (&numerator % &denominator)
                        .to_u128()
                        .expect("resto local cabe em u128"),
                )
            }
        };
        let propagated = radius_floor + u128::from(radius_remainder != 0);
        let uncapped_radius = propagated + u128::from(center_remainder != 0);
        let radius_capped = uncapped_radius >= scale;
        let output_radius = if radius_capped {
            scale
        } else {
            uncapped_radius
        };
        assert!(threshold_numerator > 0 && threshold_numerator <= scale);
        let decision_half = if output >= threshold_numerator + output_radius {
            1
        } else if output + output_radius < threshold_numerator {
            0
        } else {
            2
        };

        Self {
            input_limbs,
            input_is_one,
            input_radius,
            center_floor,
            center_remainder,
            center_rounded_up,
            output_limbs,
            output_is_one,
            center_above_half,
            center_distance,
            radius_floor,
            radius_remainder,
            uncapped_radius,
            output_radius,
            radius_capped,
            decision_half,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CbdLogisticStepCircuit<const K: usize> {
    pub witness: CbdStepWitness<K>,
    pub r_num: u64,
    pub r_den: u64,
    pub radius_bound: CbdRadiusBound,
    /// Limiar público do predicado, no numerador da escala `2^(16K)`.
    pub threshold_numerator: u128,
}

impl<const K: usize> CbdLogisticStepCircuit<K> {
    pub fn arity_value() -> usize {
        K + 4 // iteration, K limbs, is_one, radius, decision(x>=1/2)
    }
}

/// Testemunha da ablação CBD com quantização após `x*(1-x)` e após `r*p`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CbdSequentialStepWitness<const K: usize> {
    pub input_limbs: [u64; K],
    pub input_is_one: bool,
    pub input_radius: u128,
    pub center_above_half: bool,
    pub center_distance: u128,
    pub product_floor: u128,
    pub product_remainder: u128,
    pub product_rounded_up: bool,
    pub product_limbs: [u64; K],
    pub product_is_one: bool,
    pub product_radius_floor: u128,
    pub product_radius_remainder: u128,
    pub product_uncapped_radius: u128,
    pub product_radius: u128,
    pub product_radius_capped: bool,
    pub output_floor: u128,
    pub output_remainder: u128,
    pub output_rounded_up: bool,
    pub output_limbs: [u64; K],
    pub output_is_one: bool,
    pub output_radius_floor: u128,
    pub output_radius_remainder: u128,
    pub output_uncapped_radius: u128,
    pub output_radius: u128,
    pub output_radius_capped: bool,
    /// 0=false, 1=true, 2=indeterminate para `x>=t`, com `t` público.
    pub decision_half: u64,
}

impl<const K: usize> CbdSequentialStepWitness<K> {
    pub fn compute(
        input_limbs: [u64; K],
        input_is_one: bool,
        input_radius: u128,
        r_num: u64,
        r_den: u64,
    ) -> Self {
        Self::compute_with_threshold(
            input_limbs,
            input_is_one,
            input_radius,
            r_num,
            r_den,
            half_threshold_numerator(K),
        )
    }

    /// Variante do protocolo de Pareto: o limiar é um parâmetro público.
    pub fn compute_with_threshold(
        input_limbs: [u64; K],
        input_is_one: bool,
        input_radius: u128,
        r_num: u64,
        r_den: u64,
        threshold_numerator: u128,
    ) -> Self {
        assert!(K > 0 && K <= 4);
        assert!(r_den > 0);
        let fractional_bits = 16 * K;
        let scale = 1_u128 << fractional_bits;
        assert!(input_radius <= scale);
        let input = if input_is_one {
            scale
        } else {
            input_limbs
                .iter()
                .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
        };

        let raw_product = BigUint::from(input) * BigUint::from(scale - input);
        let scale_big = BigUint::from(scale);
        let product_floor = (&raw_product / &scale_big)
            .to_u128()
            .expect("produto CBD cabe em u128");
        let product_remainder = (&raw_product % &scale_big)
            .to_u128()
            .expect("resto do produto CBD cabe em u128");
        let product_rounded_up = should_round_up_u128(
            product_floor,
            product_remainder,
            scale,
            CircuitRounding::NearestTiesToEven,
        );
        let product = product_floor + u128::from(product_rounded_up);
        let product_is_one = product == scale;
        let product_limbs = split_limbs::<K>(if product_is_one { 0 } else { product });

        let center_above_half = input.saturating_mul(2) >= scale;
        let center_distance = if center_above_half {
            input * 2 - scale
        } else {
            scale - input * 2
        };
        let radius = BigUint::from(input_radius);
        let product_error = &radius * BigUint::from(center_distance) + &radius * &radius;
        let product_radius_floor = (&product_error / &scale_big)
            .to_u128()
            .expect("raio intermediário cabe em u128");
        let product_radius_remainder = (&product_error % &scale_big)
            .to_u128()
            .expect("resto do raio intermediário cabe em u128");
        let product_uncapped_radius = product_radius_floor
            + u128::from(product_radius_remainder != 0)
            + u128::from(product_remainder != 0);
        let product_radius_capped = product_uncapped_radius >= scale;
        let product_radius = if product_radius_capped {
            scale
        } else {
            product_uncapped_radius
        };

        let scaled_product = BigUint::from(product) * BigUint::from(r_num);
        let r_den_big = BigUint::from(r_den);
        let output_floor = (&scaled_product / &r_den_big)
            .to_u128()
            .expect("centro de saída cabe em u128");
        let output_remainder = (&scaled_product % &r_den_big)
            .to_u128()
            .expect("resto de saída cabe em u128");
        let output_rounded_up = should_round_up_u128(
            output_floor,
            output_remainder,
            r_den as u128,
            CircuitRounding::NearestTiesToEven,
        );
        let output = output_floor + u128::from(output_rounded_up);
        assert!(output <= scale, "mapa logístico saiu de [0,1]");
        let output_is_one = output == scale;
        let output_limbs = split_limbs::<K>(if output_is_one { 0 } else { output });

        let output_radius_numerator = BigUint::from(product_radius) * BigUint::from(r_num);
        let output_radius_floor = (&output_radius_numerator / &r_den_big)
            .to_u128()
            .expect("raio de saída cabe em u128");
        let output_radius_remainder = (&output_radius_numerator % &r_den_big)
            .to_u128()
            .expect("resto do raio de saída cabe em u128");
        let output_uncapped_radius = output_radius_floor
            + u128::from(output_radius_remainder != 0)
            + u128::from(output_remainder != 0);
        let output_radius_capped = output_uncapped_radius >= scale;
        let output_radius = if output_radius_capped {
            scale
        } else {
            output_uncapped_radius
        };
        assert!(threshold_numerator > 0 && threshold_numerator <= scale);
        let decision_half = if output >= threshold_numerator + output_radius {
            1
        } else if output + output_radius < threshold_numerator {
            0
        } else {
            2
        };

        Self {
            input_limbs,
            input_is_one,
            input_radius,
            center_above_half,
            center_distance,
            product_floor,
            product_remainder,
            product_rounded_up,
            product_limbs,
            product_is_one,
            product_radius_floor,
            product_radius_remainder,
            product_uncapped_radius,
            product_radius,
            product_radius_capped,
            output_floor,
            output_remainder,
            output_rounded_up,
            output_limbs,
            output_is_one,
            output_radius_floor,
            output_radius_remainder,
            output_uncapped_radius,
            output_radius,
            output_radius_capped,
            decision_half,
        }
    }
}

/// Decompõe um numerador na escala `2^(16K)` em `K` limbs de 16 bits.
pub fn split_limbs<const K: usize>(mut value: u128) -> [u64; K] {
    let mut limbs = [0_u64; K];
    for limb in limbs.iter_mut().rev() {
        *limb = (value & 0xffff) as u64;
        value >>= 16;
    }
    debug_assert_eq!(value, 0);
    limbs
}

#[derive(Clone, Debug)]
pub struct CbdSequentialLogisticStepCircuit<const K: usize> {
    pub witness: CbdSequentialStepWitness<K>,
    pub r_num: u64,
    pub r_den: u64,
    /// Limiar público do predicado, no numerador da escala `2^(16K)`.
    pub threshold_numerator: u128,
}

impl<const K: usize> CbdSequentialLogisticStepCircuit<K> {
    pub const fn arity_value() -> usize {
        K + 4
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductCarryWitness<const K: usize> {
    pub complement_limbs: [u64; K],
    pub complement_is_one: bool,
    pub pair_products_le: Vec<u128>,
    pub product_digits_le: Vec<u64>,
    pub carries: Vec<u128>,
}

pub type ProductCarryWitness2 = ProductCarryWitness<2>;

impl<const K: usize> ProductCarryWitness<K> {
    pub fn compute(input_limbs: [u64; K], input_is_one: bool) -> Self {
        assert!(K > 0 && K <= 4);
        let base = 1_u128 << 16;
        let scale = 1_u128 << (16 * K);
        let input = if input_is_one {
            scale
        } else {
            input_limbs
                .iter()
                .fold(0_u128, |acc, &limb| (acc << 16) + limb as u128)
        };
        let complement = scale - input;
        let complement_is_one = complement == scale;
        let mut complement_limbs = [0_u64; K];
        if !complement_is_one {
            let mut remaining = complement;
            for limb in complement_limbs.iter_mut().rev() {
                *limb = (remaining & 0xffff) as u64;
                remaining >>= 16;
            }
        }
        let input_le = input_limbs
            .iter()
            .rev()
            .map(|limb| *limb as u128)
            .collect::<Vec<_>>();
        let complement_le = complement_limbs
            .iter()
            .rev()
            .map(|limb| *limb as u128)
            .collect::<Vec<_>>();
        let mut pair_products_le = Vec::with_capacity(K * K);
        for &a in &input_le {
            for &b in &complement_le {
                pair_products_le.push(a * b);
            }
        }

        let mut product_digits_le = Vec::with_capacity(2 * K);
        let mut carries = Vec::with_capacity(2 * K - 1);
        let mut carry = 0_u128;
        for column in 0..(2 * K - 1) {
            let mut sum = carry;
            for i in 0..K {
                if column >= i {
                    let j = column - i;
                    if j < K {
                        sum += pair_products_le[i * K + j];
                    }
                }
            }
            product_digits_le.push((sum % base) as u64);
            carry = sum / base;
            carries.push(carry);
        }
        assert!(carry < base);
        product_digits_le.push(carry as u64);

        Self {
            complement_limbs,
            complement_is_one,
            pair_products_le,
            product_digits_le,
            carries,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CarryLogisticStepCircuit<const K: usize> {
    pub witness: StepWitness<K>,
    pub carry_witness: ProductCarryWitness<K>,
    pub r_num: u64,
    pub r_den: u64,
    pub rounding: CircuitRounding,
    /// Limiar público do predicado, no numerador da escala `2^(16K)`.
    pub threshold_numerator: u128,
}

pub type CarryLogisticStepCircuit2 = CarryLogisticStepCircuit<2>;

impl<const K: usize> CarryLogisticStepCircuit<K> {
    pub fn arity_value() -> usize {
        LogisticStepCircuit::<K>::arity_value()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompensatedStepWitness {
    pub input_limbs: [u64; 2],
    pub input_is_one: bool,
    /// Codifica `E + S/2`, onde `E` é o residual assinado.
    pub input_residual_offset: u64,
    pub fine_floor: u128,
    pub fine_remainder: u128,
    pub fine_rounded_up: bool,
    pub output_limbs: [u64; 2],
    pub output_is_one: bool,
    pub output_residual_offset: u64,
}

impl CompensatedStepWitness {
    pub fn compute(
        input_limbs: [u64; 2],
        input_is_one: bool,
        input_residual_offset: u64,
        r_num: u64,
        r_den: u64,
    ) -> Self {
        Self::compute_with_rounding(
            input_limbs,
            input_is_one,
            input_residual_offset,
            r_num,
            r_den,
            CircuitRounding::Truncate,
        )
    }

    pub fn compute_with_rounding(
        input_limbs: [u64; 2],
        input_is_one: bool,
        input_residual_offset: u64,
        r_num: u64,
        r_den: u64,
        rounding: CircuitRounding,
    ) -> Self {
        assert!(r_den > 0);
        let scale = 1_u128 << 32;
        let half = scale / 2;
        assert!(input_residual_offset as u128 <= scale);
        let input_num = if input_is_one {
            scale
        } else {
            ((input_limbs[0] as u128) << 16) + input_limbs[1] as u128
        };
        let residual = input_residual_offset as i128 - half as i128;
        let input_fine = (input_num * scale) as i128 + residual;
        assert!((0..=(scale * scale) as i128).contains(&input_fine));
        let input_fine = input_fine as u128;
        let fine_scale = scale * scale;
        let product = BigUint::from(input_fine) * BigUint::from(fine_scale - input_fine);
        let raw = product * BigUint::from(r_num);
        let divisor = BigUint::from(r_den) * BigUint::from(fine_scale);
        let fine_floor = (&raw / &divisor)
            .to_u128()
            .expect("resultado fino cabe em u128 para K=2");
        let fine_remainder = (&raw % &divisor)
            .to_u128()
            .expect("resto fino cabe em u128 para K=2");
        let divisor_u128 = divisor
            .to_u128()
            .expect("divisor fino cabe em u128 para K=2");
        let fine_rounded_up =
            should_round_up_u128(fine_floor, fine_remainder, divisor_u128, rounding);
        let fine_output = fine_floor + u128::from(fine_rounded_up);

        let mut main = (fine_output + half) / scale;
        if main > scale {
            main = scale;
        }
        let output_residual = fine_output as i128 - (main * scale) as i128;
        assert!((-(half as i128)..=(half as i128)).contains(&output_residual));
        let output_residual_offset = (output_residual + half as i128) as u64;
        let output_is_one = main == scale;
        let remaining = if output_is_one { 0 } else { main };
        let output_limbs = [(remaining >> 16) as u64, (remaining & 0xffff) as u64];

        Self {
            input_limbs,
            input_is_one,
            input_residual_offset,
            fine_floor,
            fine_remainder,
            fine_rounded_up,
            output_limbs,
            output_is_one,
            output_residual_offset,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompensatedLogisticStepCircuit {
    pub witness: CompensatedStepWitness,
    pub r_num: u64,
    pub r_den: u64,
    pub rounding: CircuitRounding,
    /// Limiar público do predicado, no numerador da grade fina `2^64`.
    pub threshold_fine_numerator: u128,
}

impl CompensatedLogisticStepCircuit {
    pub fn arity_value() -> usize {
        6 // iteration, 2 limbs, is_one, residual_offset, decision
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConstraintBreakdown {
    pub range_checks: usize,
    pub multiplication_relations: usize,
    pub division_relations: usize,
    pub canonicality: usize,
    pub counter: usize,
}

impl ConstraintBreakdown {
    pub fn modeled_total(self) -> usize {
        self.range_checks
            + self.multiplication_relations
            + self.division_relations
            + self.canonicality
            + self.counter
    }
}

impl<const K: usize> LogisticStepCircuit<K> {
    pub fn arity_value() -> usize {
        K + 3 // iteration, K limbs, is_one, decision
    }

    pub fn breakdown(&self) -> ConstraintBreakdown {
        let scale_bits = 16 * K;
        let remainder_scale_bits =
            (u64::BITS - self.r_den.saturating_sub(1).leading_zeros()) as usize;
        ConstraintBreakdown {
            // Cada bit alocado custa uma constraint booleana; cada decomposição
            // adiciona uma igualdade de reconstrução.
            range_checks: 2 * K * (16 + 1)
                + (scale_bits + 1)
                + 1
                + scale_bits
                + 1
                + (scale_bits + 1)
                + 1
                + remainder_scale_bits
                + 1,
            multiplication_relations: 1,
            division_relations: 2,
            canonicality: 2 * (K + 1) + 2,
            counter: 1,
        }
    }
}

/// Quantiza um racional não negativo numa grade diádica de `fractional_bits`.
///
/// Devolve o numerador escolhido e se a quantização foi inexata. O segundo
/// valor é o raio inicial de um estado CBD: sem ele, o primeiro passo alegaria
/// conhecer `x0` exatamente quando `x0` não é diádico.
///
/// Vive na biblioteca, e não no binário de custo, porque o binário de cobertura
/// precisa partir exatamente do mesmo estado inicial. Duas quantizações
/// diferentes produziriam trajetórias diferentes sob o mesmo `profile_hash_hex`.
pub fn quantize_dyadic(
    value: &BigRational,
    fractional_bits: usize,
    rounding: CircuitRounding,
) -> (u128, bool) {
    let scale = BigUint::one() << fractional_bits;
    let numerator = value
        .numer()
        .to_biguint()
        .expect("valor não negativo por validação");
    let denominator = value
        .denom()
        .to_biguint()
        .expect("denominador positivo por construção");
    let raw = numerator * &scale;
    let floor = (&raw / &denominator)
        .to_u128()
        .expect("piso da quantização cabe em u128");
    let remainder = (&raw % &denominator)
        .to_u128()
        .expect("resto da quantização cabe em u128");
    let denominator = denominator
        .to_u128()
        .expect("denominador cabe em u128");
    let rounded_up = should_round_up_u128(floor, remainder, denominator, rounding);
    (floor + u128::from(rounded_up), remainder != 0)
}

/// Estado inicial do ponto fixo: limbs de `x0` na grade do perfil.
pub fn initial_fixed_point_state<const K: usize>(
    x0: &BigRational,
    rounding: CircuitRounding,
) -> ([u64; K], bool) {
    let (numerator, _) = quantize_dyadic(x0, 16 * K, rounding);
    let scale = 1_u128 << (16 * K);
    if numerator == scale {
        ([0; K], true)
    } else {
        (split_limbs::<K>(numerator), false)
    }
}

/// Estado inicial CBD: limbs de `x0` mais o raio de uma ULP quando `x0` não é
/// representável na grade.
pub fn initial_cbd_state<const K: usize>(x0: &BigRational) -> ([u64; K], bool, u128) {
    let (numerator, inexact) = quantize_dyadic(x0, 16 * K, CircuitRounding::NearestTiesToEven);
    let scale = 1_u128 << (16 * K);
    if numerator == scale {
        ([0; K], true, u128::from(inexact))
    } else {
        (split_limbs::<K>(numerator), false, u128::from(inexact))
    }
}

/// Estado inicial compensado: par `(V, E)` na grade fina `S²`, com `K = 2`.
pub fn initial_compensated_state(
    x0: &BigRational,
    rounding: CircuitRounding,
) -> ([u64; 2], bool, u64) {
    let scale = 1_u128 << 32;
    let (fine, _) = quantize_dyadic(x0, 64, rounding);
    let half = scale / 2;
    let mut main = (fine + half) / scale;
    if main > scale {
        main = scale;
    }
    let residual = fine as i128 - (main * scale) as i128;
    let offset = (residual + half as i128) as u64;
    let is_one = main == scale;
    let remaining = if is_one { 0 } else { main };
    (
        [(remaining >> 16) as u64, (remaining & 0xffff) as u64],
        is_one,
        offset,
    )
}

/// Numerador do limiar público na escala `2^(16K)`.
///
/// Devolve `None` quando o limiar não cai exatamente na grade: o gadget de
/// decisão compara inteiros, e arredondar o limiar em silêncio mudaria o
/// predicado provado.
pub fn fixed_point_threshold_numerator<const K: usize>(threshold: &BigRational) -> Option<u128> {
    let scale = BigUint::one() << (16 * K);
    let scaled = threshold * BigRational::from_integer(scale.into());
    if !scaled.is_integer() {
        return None;
    }
    scaled.to_integer().to_u128()
}

fn should_round_up_u128(
    quotient: u128,
    remainder: u128,
    denominator: u128,
    mode: CircuitRounding,
) -> bool {
    if remainder == 0 || mode == CircuitRounding::Truncate {
        return false;
    }
    match (remainder * 2).cmp(&denominator) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => quotient & 1 == 1,
    }
}

pub(crate) fn field_from_u128<F: PrimeField>(value: u128) -> F {
    let high = F::from((value >> 64) as u64);
    let mut two64 = F::ONE;
    for _ in 0..64 {
        two64 = two64.double();
    }
    high * two64 + F::from(value as u64)
}

fn bit_length_u128(value: u128) -> usize {
    if value == 0 {
        1
    } else {
        (u128::BITS - value.leading_zeros()) as usize
    }
}

pub(crate) fn range_check<F, CS>(
    mut cs: CS,
    number: &AllocatedNum<F>,
    witness: u128,
    bits: usize,
) -> Result<Vec<AllocatedBit>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let allocated = (0..bits)
        .map(|i| {
            AllocatedBit::alloc(
                cs.namespace(|| format!("bit_{i}")),
                Some(((witness >> i) & 1) == 1),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut reconstruction = LinearCombination::zero();
    let mut coefficient = F::ONE;
    for bit in &allocated {
        reconstruction = reconstruction + (coefficient, bit.get_variable());
        coefficient = coefficient.double();
    }
    reconstruction = reconstruction - number.get_variable();
    cs.enforce(
        || "reconstruct ranged number",
        |lc| lc,
        |lc| lc,
        |_| reconstruction,
    );
    Ok(allocated)
}

pub(crate) fn allocate_ranged<F, CS>(
    mut cs: CS,
    name: &'static str,
    witness: u128,
    bits: usize,
) -> Result<(AllocatedNum<F>, Vec<AllocatedBit>), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let number = AllocatedNum::alloc(cs.namespace(|| name), || Ok(field_from_u128(witness)))?;
    let bits = range_check(
        cs.namespace(|| format!("{name}_range")),
        &number,
        witness,
        bits,
    )?;
    Ok((number, bits))
}

fn enforce_boolean_num<F, CS>(mut cs: CS, number: &AllocatedNum<F>)
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    cs.enforce(
        || "boolean number",
        |lc| lc + number.get_variable(),
        |lc| lc + CS::one() - number.get_variable(),
        |lc| lc,
    );
}

/// Prova o predicado público `valor >= limiar` para um valor sem raio.
///
/// Ponto fixo e FP32 sempre respondem: não há ramo indeterminado. As duas
/// diferenças com range check são o que amarra a decisão ao valor — no ramo
/// errado, a diferença exigida seria negativa e nenhum inteiro de
/// `comparison_bits` bits a satisfaz.
///
/// O gadget existe para que o custo do ponto fixo inclua a decisão, como já
/// acontece em CBD, CFD e FP32. Sem ele, o custo por decisão do protocolo
/// compararia um circuito que decide contra um circuito que só itera.
fn enforce_threshold_decision<F, CS>(
    mut cs: CS,
    name: &'static str,
    value: &AllocatedNum<F>,
    value_witness: u128,
    threshold: u128,
    comparison_bits: usize,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    assert!(threshold > 0);
    let decided_true = value_witness >= threshold;
    let decision = allocate_boolean_num(cs.namespace(|| "decision"), "decision_value", decided_true)?;
    let true_diff_witness = if decided_true {
        value_witness - threshold
    } else {
        0
    };
    let false_diff_witness = if decided_true {
        0
    } else {
        threshold - 1 - value_witness
    };
    let (true_diff, _) = allocate_ranged(
        cs.namespace(|| "true_diff"),
        "true_diff_value",
        true_diff_witness,
        comparison_bits,
    )?;
    let (false_diff, _) = allocate_ranged(
        cs.namespace(|| "false_diff"),
        "false_diff_value",
        false_diff_witness,
        comparison_bits,
    )?;
    let threshold_field = field_from_u128::<F>(threshold);
    cs.enforce(
        || format!("{name} true proves value >= threshold"),
        |lc| lc + decision.get_variable(),
        |lc| {
            lc + value.get_variable() - (threshold_field, CS::one()) - true_diff.get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || format!("{name} false proves value < threshold"),
        |lc| lc + CS::one() - decision.get_variable(),
        |lc| {
            lc + (threshold_field, CS::one())
                - CS::one()
                - value.get_variable()
                - false_diff.get_variable()
        },
        |lc| lc,
    );
    Ok(decision)
}

pub(crate) struct CertifiedDecisionArgs<'a, F: PrimeField> {
    pub center: &'a AllocatedNum<F>,
    pub center_witness: u128,
    pub radius: &'a AllocatedNum<F>,
    pub radius_witness: u128,
    pub threshold: u128,
    /// 0=falso, 1=verdadeiro, 2=indeterminado.
    pub decision_witness: u64,
    pub comparison_bits: usize,
}

/// Prova a decisão de três valores de um enclosure `[centro-raio, centro+raio]`
/// contra o limiar público.
///
/// As quatro diferenças com range check amarram os dois ramos nos **dois**
/// sentidos: `verdadeiro` exige `centro - raio >= t` e `não verdadeiro` exige
/// `centro - raio < t`. Sem o segundo par, um provador poderia declarar
/// `indeterminado` num passo em que o enclosure decide, e a abstenção deixaria
/// de ser informação sobre o enclosure para virar escolha do provador — o que
/// destruiria a métrica de cobertura do protocolo.
pub(crate) fn enforce_certified_decision<F, CS>(
    mut cs: CS,
    name: &str,
    args: CertifiedDecisionArgs<'_, F>,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let is_true = args.decision_witness == 1;
    let is_false = args.decision_witness == 0;
    let is_indeterminate = args.decision_witness == 2;
    let decision_true = allocate_boolean_num(
        cs.namespace(|| "decision_true"),
        "decision_true_value",
        is_true,
    )?;
    let decision_false = allocate_boolean_num(
        cs.namespace(|| "decision_false"),
        "decision_false_value",
        is_false,
    )?;
    let decision_indeterminate = allocate_boolean_num(
        cs.namespace(|| "decision_indeterminate"),
        "decision_indeterminate_value",
        is_indeterminate,
    )?;
    cs.enforce(
        || format!("{name} decision one-hot"),
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + decision_true.get_variable()
                + decision_false.get_variable()
                + decision_indeterminate.get_variable()
                - CS::one()
        },
    );

    let threshold = field_from_u128::<F>(args.threshold);
    let bits = args.comparison_bits;
    let true_diff_witness = if is_true {
        args.center_witness
            .saturating_sub(args.radius_witness)
            .saturating_sub(args.threshold)
    } else {
        0
    };
    let not_true_diff_witness = if is_true {
        0
    } else {
        (args.threshold + args.radius_witness)
            .saturating_sub(1)
            .saturating_sub(args.center_witness)
    };
    let false_diff_witness = if is_false {
        args.threshold
            .saturating_sub(1)
            .saturating_sub(args.center_witness)
            .saturating_sub(args.radius_witness)
    } else {
        0
    };
    let not_false_diff_witness = if is_false {
        0
    } else {
        (args.center_witness + args.radius_witness).saturating_sub(args.threshold)
    };
    let (true_diff, _) = allocate_ranged(
        cs.namespace(|| "true_diff"),
        "true_diff_value",
        true_diff_witness,
        bits,
    )?;
    let (not_true_diff, _) = allocate_ranged(
        cs.namespace(|| "not_true_diff"),
        "not_true_diff_value",
        not_true_diff_witness,
        bits,
    )?;
    let (false_diff, _) = allocate_ranged(
        cs.namespace(|| "false_diff"),
        "false_diff_value",
        false_diff_witness,
        bits,
    )?;
    let (not_false_diff, _) = allocate_ranged(
        cs.namespace(|| "not_false_diff"),
        "not_false_diff_value",
        not_false_diff_witness,
        bits,
    )?;
    cs.enforce(
        || format!("{name} true proves lower >= threshold"),
        |lc| lc + decision_true.get_variable(),
        |lc| {
            lc + args.center.get_variable()
                - args.radius.get_variable()
                - (threshold, CS::one())
                - true_diff.get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || format!("{name} not true proves lower < threshold"),
        |lc| lc + CS::one() - decision_true.get_variable(),
        |lc| {
            lc + (threshold, CS::one()) + args.radius.get_variable()
                - CS::one()
                - args.center.get_variable()
                - not_true_diff.get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || format!("{name} false proves upper < threshold"),
        |lc| lc + decision_false.get_variable(),
        |lc| {
            lc + (threshold, CS::one())
                - CS::one()
                - args.center.get_variable()
                - args.radius.get_variable()
                - false_diff.get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || format!("{name} not false proves upper >= threshold"),
        |lc| lc + CS::one() - decision_false.get_variable(),
        |lc| {
            lc + args.center.get_variable() + args.radius.get_variable()
                - (threshold, CS::one())
                - not_false_diff.get_variable()
        },
        |lc| lc,
    );

    let (decision_code, _) = allocate_ranged(
        cs.namespace(|| "decision_code"),
        "decision_code_value",
        args.decision_witness as u128,
        2,
    )?;
    cs.enforce(
        || format!("{name} decision code"),
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + decision_true.get_variable()
                + (F::from(2), decision_indeterminate.get_variable())
                - decision_code.get_variable()
        },
    );
    Ok(decision_code)
}

fn allocate_boolean_num<F, CS>(
    mut cs: CS,
    name: &'static str,
    value: bool,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let number = AllocatedNum::alloc(cs.namespace(|| name), || {
        Ok(if value { F::ONE } else { F::ZERO })
    })?;
    enforce_boolean_num(cs.namespace(|| format!("{name}_boolean")), &number);
    Ok(number)
}

fn enforce_zero_num<F, CS>(mut cs: CS, name: &'static str, number: &AllocatedNum<F>)
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    cs.enforce(|| name, |lc| lc, |lc| lc, |lc| lc + number.get_variable());
}

fn allocated_bit_value(bit: &AllocatedBit) -> Result<bool, SynthesisError> {
    bit.get_value().ok_or(SynthesisError::AssignmentMissing)
}

fn lower_bits_nonzero_witness(bits: &[AllocatedBit]) -> Result<(bool, u128), SynthesisError> {
    let mut sum = 0_u128;
    for (index, bit) in bits.iter().enumerate() {
        if allocated_bit_value(bit)? {
            sum += 1_u128 << index;
        }
    }
    Ok((sum != 0, sum))
}

fn allocate_nonzero_indicator_from_bits<F, CS>(
    mut cs: CS,
    bits: &[AllocatedBit],
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let (nonzero, sum_witness) = lower_bits_nonzero_witness(bits)?;
    let indicator = allocate_boolean_num(cs.namespace(|| "nonzero"), "nonzero_value", nonzero)?;
    let inverse = AllocatedNum::alloc(cs.namespace(|| "nonzero_inverse"), || {
        if sum_witness == 0 {
            Ok(F::ZERO)
        } else {
            Option::<F>::from(field_from_u128::<F>(sum_witness).invert())
                .ok_or_else(|| SynthesisError::Unsatisfiable("nonzero inverse".to_owned()))
        }
    })?;

    let mut sum_lc = LinearCombination::zero();
    let mut coefficient = F::ONE;
    for bit in bits {
        sum_lc = sum_lc + (coefficient, bit.get_variable());
        coefficient = coefficient.double();
    }
    cs.enforce(
        || "sum * inverse = nonzero",
        |_| sum_lc.clone(),
        |lc| lc + inverse.get_variable(),
        |lc| lc + indicator.get_variable(),
    );
    cs.enforce(
        || "sum is zero when nonzero is false",
        |_| sum_lc,
        |lc| lc + CS::one() - indicator.get_variable(),
        |lc| lc,
    );
    Ok(indicator)
}

fn enforce_rounding_power_of_two<F, CS>(
    mut cs: CS,
    mode: CircuitRounding,
    floor_witness: u128,
    remainder_witness: u128,
    floor_bits: &[AllocatedBit],
    remainder_bits: &[AllocatedBit],
    round_up: &AllocatedNum<F>,
) -> Result<(), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    match mode {
        CircuitRounding::Truncate => {
            enforce_zero_num(
                cs.namespace(|| "truncate_round_up_zero"),
                "round_up_zero",
                round_up,
            );
        }
        CircuitRounding::NearestTiesToEven => {
            let Some(ge_half) = remainder_bits.last() else {
                enforce_zero_num(
                    cs.namespace(|| "empty_remainder_zero"),
                    "round_up_zero",
                    round_up,
                );
                return Ok(());
            };
            let lower_nonzero = allocate_nonzero_indicator_from_bits(
                cs.namespace(|| "remainder_lower_nonzero"),
                &remainder_bits[..remainder_bits.len() - 1],
            )?;
            let floor_odd = floor_bits
                .first()
                .ok_or_else(|| SynthesisError::Unsatisfiable("missing floor parity".to_owned()))?;
            let half = 1_u128 << (remainder_bits.len() - 1);
            let round_after_half_witness =
                (remainder_witness % half != 0) || (floor_witness & 1 == 1);
            let round_after_half = allocate_boolean_num(
                cs.namespace(|| "round_after_half"),
                "round_after_half_value",
                round_after_half_witness,
            )?;
            cs.enforce(
                || "round_after_half is lower_nonzero OR floor_odd",
                |lc| lc + lower_nonzero.get_variable(),
                |lc| lc + floor_odd.get_variable(),
                |lc| {
                    lc + lower_nonzero.get_variable() + floor_odd.get_variable()
                        - round_after_half.get_variable()
                },
            );
            cs.enforce(
                || "round_up = ge_half AND round_after_half",
                |lc| lc + ge_half.get_variable(),
                |lc| lc + round_after_half.get_variable(),
                |lc| lc + round_up.get_variable(),
            );
        }
    }
    Ok(())
}

struct SmallRemainderRounding<'a, F: PrimeField> {
    floor_witness: u128,
    floor_bits: &'a [AllocatedBit],
    remainder: &'a AllocatedNum<F>,
    remainder_witness: u64,
    divisor: u64,
    round_up: &'a AllocatedNum<F>,
}

fn enforce_small_remainder_rounding<F, CS>(
    mut cs: CS,
    mode: CircuitRounding,
    args: SmallRemainderRounding<'_, F>,
) -> Result<(), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let eqs = (0..args.divisor)
        .map(|candidate| {
            let eq = allocate_boolean_num(
                cs.namespace(|| format!("remainder_eq_{candidate}")),
                "eq_value",
                args.remainder_witness == candidate,
            )?;
            cs.enforce(
                || format!("eq_{candidate}_implies_remainder"),
                |lc| lc + eq.get_variable(),
                |lc| lc + args.remainder.get_variable() - (F::from(candidate), CS::one()),
                |lc| lc,
            );
            Ok(eq)
        })
        .collect::<Result<Vec<_>, SynthesisError>>()?;

    cs.enforce(
        || "small remainder one-hot",
        |lc| lc,
        |lc| lc,
        |lc| {
            eqs.iter()
                .fold(lc - CS::one(), |acc, eq| acc + eq.get_variable())
        },
    );

    match mode {
        CircuitRounding::Truncate => {
            enforce_zero_num(
                cs.namespace(|| "truncate_small_round_up_zero"),
                "round_up_zero",
                args.round_up,
            );
        }
        CircuitRounding::NearestTiesToEven => {
            let floor_odd = args
                .floor_bits
                .first()
                .ok_or_else(|| SynthesisError::Unsatisfiable("missing floor parity".to_owned()))?;
            let mut greater_than_half = LinearCombination::zero();
            let mut tie_eq = None;
            for (candidate, eq) in eqs.iter().enumerate() {
                let doubled = 2 * candidate as u64;
                match doubled.cmp(&args.divisor) {
                    std::cmp::Ordering::Greater => {
                        greater_than_half = greater_than_half + eq.get_variable();
                    }
                    std::cmp::Ordering::Equal => {
                        tie_eq = Some(eq);
                    }
                    std::cmp::Ordering::Less => {}
                }
            }

            if let Some(eq) = tie_eq {
                let tie_round = allocate_boolean_num(
                    cs.namespace(|| "small_tie_round"),
                    "small_tie_round_value",
                    args.remainder_witness * 2 == args.divisor && args.floor_witness & 1 == 1,
                )?;
                cs.enforce(
                    || "small tie rounds iff floor is odd",
                    |lc| lc + eq.get_variable(),
                    |lc| lc + floor_odd.get_variable(),
                    |lc| lc + tie_round.get_variable(),
                );
                cs.enforce(
                    || "small divisor round decision",
                    |lc| lc,
                    |lc| lc,
                    |_| greater_than_half + tie_round.get_variable() - args.round_up.get_variable(),
                );
            } else {
                cs.enforce(
                    || "small divisor round decision",
                    |lc| lc,
                    |lc| lc,
                    |_| greater_than_half - args.round_up.get_variable(),
                );
            }
        }
    }
    Ok(())
}

struct ConstantDivisorRoundingArgs<'a, F: PrimeField> {
    mode: CircuitRounding,
    floor_witness: u128,
    remainder_witness: u128,
    divisor: u128,
    floor_bits: &'a [AllocatedBit],
    remainder: &'a AllocatedNum<F>,
    round_up: &'a AllocatedNum<F>,
}

fn enforce_constant_divisor_rounding<F, CS>(
    mut cs: CS,
    args: ConstantDivisorRoundingArgs<'_, F>,
) -> Result<(), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    match args.mode {
        CircuitRounding::Truncate => {
            enforce_zero_num(
                cs.namespace(|| "truncate_constant_round_up_zero"),
                "round_up_zero",
                args.round_up,
            );
        }
        CircuitRounding::NearestTiesToEven => {
            let doubled = args.remainder_witness * 2;
            let is_less = doubled < args.divisor;
            let is_equal = doubled == args.divisor;
            let is_greater = doubled > args.divisor;
            let less =
                allocate_boolean_num(cs.namespace(|| "constant_round_less"), "less", is_less)?;
            let equal =
                allocate_boolean_num(cs.namespace(|| "constant_round_equal"), "equal", is_equal)?;
            let greater = allocate_boolean_num(
                cs.namespace(|| "constant_round_greater"),
                "greater",
                is_greater,
            )?;
            cs.enforce(
                || "constant round comparison one-hot",
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + less.get_variable() + equal.get_variable() + greater.get_variable()
                        - CS::one()
                },
            );

            let diff_bits = bit_length_u128(args.divisor);
            let less_diff_witness = if is_less {
                args.divisor - 1 - doubled
            } else {
                0
            };
            let greater_diff_witness = if is_greater {
                doubled - args.divisor - 1
            } else {
                0
            };
            let (less_diff, _) = allocate_ranged(
                cs.namespace(|| "constant_round_less_diff"),
                "less_diff",
                less_diff_witness,
                diff_bits,
            )?;
            let (greater_diff, _) = allocate_ranged(
                cs.namespace(|| "constant_round_greater_diff"),
                "greater_diff",
                greater_diff_witness,
                diff_bits,
            )?;
            cs.enforce(
                || "less selector proves 2r < divisor",
                |lc| lc + less.get_variable(),
                |lc| {
                    lc + (F::from(2), args.remainder.get_variable())
                        + less_diff.get_variable()
                        + CS::one()
                        - (field_from_u128::<F>(args.divisor), CS::one())
                },
                |lc| lc,
            );
            cs.enforce(
                || "equal selector proves 2r = divisor",
                |lc| lc + equal.get_variable(),
                |lc| {
                    lc + (F::from(2), args.remainder.get_variable())
                        - (field_from_u128::<F>(args.divisor), CS::one())
                },
                |lc| lc,
            );
            cs.enforce(
                || "greater selector proves 2r > divisor",
                |lc| lc + greater.get_variable(),
                |lc| {
                    lc + (field_from_u128::<F>(args.divisor), CS::one())
                        + greater_diff.get_variable()
                        + CS::one()
                        - (F::from(2), args.remainder.get_variable())
                },
                |lc| lc,
            );

            let floor_odd = args
                .floor_bits
                .first()
                .ok_or_else(|| SynthesisError::Unsatisfiable("missing floor parity".to_owned()))?;
            let tie_round = allocate_boolean_num(
                cs.namespace(|| "constant_tie_round"),
                "constant_tie_round_value",
                is_equal && args.floor_witness & 1 == 1,
            )?;
            cs.enforce(
                || "constant tie rounds iff floor is odd",
                |lc| lc + equal.get_variable(),
                |lc| lc + floor_odd.get_variable(),
                |lc| lc + tie_round.get_variable(),
            );
            cs.enforce(
                || "constant round decision",
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + greater.get_variable() + tie_round.get_variable()
                        - args.round_up.get_variable()
                },
            );
        }
    }
    Ok(())
}

impl<F: PrimeField, const K: usize> StepCircuit<F> for LogisticStepCircuit<K> {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert!(K > 0 && K <= 4);
        assert_eq!(z.len(), Self::arity_value());
        let scale_u128 = 1_u128 << (16 * K);
        let scale = field_from_u128::<F>(scale_u128);

        for (index, limb) in z[1..=K].iter().enumerate() {
            range_check(
                cs.namespace(|| format!("input_limb_{index}_range")),
                limb,
                self.witness.input_limbs[index] as u128,
                16,
            )?;
        }
        let input_is_one = &z[K + 1];
        enforce_boolean_num(cs.namespace(|| "input_is_one_boolean"), input_is_one);
        for (index, limb) in z[1..=K].iter().enumerate() {
            cs.enforce(
                || format!("canonical_input_one_limb_{index}"),
                |lc| lc + input_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }

        let input_num = AllocatedNum::alloc(cs.namespace(|| "input_numerator"), || {
            let value = z[1..=K]
                .iter()
                .try_fold(F::ZERO, |acc, limb| {
                    limb.get_value()
                        .map(|value| acc * F::from(1_u64 << 16) + value)
                })
                .and_then(|value| {
                    input_is_one
                        .get_value()
                        .map(|is_one| value + is_one * scale)
                });
            value.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let mut input_reconstruction = LinearCombination::zero();
        let mut coefficient = F::ONE;
        for limb in z[1..=K].iter().rev() {
            input_reconstruction = input_reconstruction + (coefficient, limb.get_variable());
            coefficient *= F::from(1_u64 << 16);
        }
        input_reconstruction =
            input_reconstruction + (scale, input_is_one.get_variable()) - input_num.get_variable();
        cs.enforce(
            || "reconstruct input numerator",
            |lc| lc,
            |lc| lc,
            |_| input_reconstruction,
        );

        let complement = AllocatedNum::alloc(cs.namespace(|| "complement"), || {
            input_num
                .get_value()
                .map(|value| scale - value)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "complement = S - input",
            |lc| lc,
            |lc| lc,
            |lc| lc + (scale, CS::one()) - input_num.get_variable() - complement.get_variable(),
        );
        let raw_product = input_num.mul(cs.namespace(|| "input_times_complement"), &complement)?;

        let (product_floor, product_floor_bits) = allocate_ranged(
            cs.namespace(|| "product_floor"),
            "product_floor_value",
            self.witness.product_floor,
            16 * K + 1,
        )?;
        let (remainder, remainder_bits) = allocate_ranged(
            cs.namespace(|| "product_remainder"),
            "product_remainder_value",
            self.witness.remainder_product,
            16 * K,
        )?;
        cs.enforce(
            || "raw_product = S * product_floor + remainder",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + raw_product.get_variable()
                    - (scale, product_floor.get_variable())
                    - remainder.get_variable()
            },
        );
        let product_round_up = allocate_boolean_num(
            cs.namespace(|| "product_round_up"),
            "product_round_up_value",
            self.witness.product_rounded_up,
        )?;
        enforce_rounding_power_of_two(
            cs.namespace(|| "product_rounding_rule"),
            self.rounding,
            self.witness.product_floor,
            self.witness.remainder_product,
            &product_floor_bits,
            &remainder_bits,
            &product_round_up,
        )?;
        let quotient = AllocatedNum::alloc(cs.namespace(|| "product_quotient"), || {
            Ok(field_from_u128(self.witness.quotient_product))
        })?;
        cs.enforce(
            || "product_quotient = product_floor + product_round_up",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + product_floor.get_variable() + product_round_up.get_variable()
                    - quotient.get_variable()
            },
        );

        let mut output_limbs = Vec::with_capacity(K);
        for (index, &limb) in self.witness.output_limbs.iter().enumerate() {
            let (limb_num, _) = allocate_ranged(
                cs.namespace(|| format!("output_limb_{index}")),
                "output_limb_value",
                limb as u128,
                16,
            )?;
            output_limbs.push(limb_num);
        }
        let output_is_one = AllocatedNum::alloc(cs.namespace(|| "output_is_one"), || {
            Ok(if self.witness.output_is_one {
                F::ONE
            } else {
                F::ZERO
            })
        })?;
        enforce_boolean_num(cs.namespace(|| "output_is_one_boolean"), &output_is_one);
        for (index, limb) in output_limbs.iter().enumerate() {
            cs.enforce(
                || format!("canonical_output_one_limb_{index}"),
                |lc| lc + output_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }
        let output_num = AllocatedNum::alloc(cs.namespace(|| "output_numerator"), || {
            let value = output_limbs.iter().try_fold(F::ZERO, |acc, limb| {
                limb.get_value()
                    .map(|value| acc * F::from(1_u64 << 16) + value)
            });
            value
                .and_then(|value| {
                    output_is_one
                        .get_value()
                        .map(|is_one| value + is_one * scale)
                })
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let mut output_reconstruction = LinearCombination::zero();
        let mut coefficient = F::ONE;
        for limb in output_limbs.iter().rev() {
            output_reconstruction = output_reconstruction + (coefficient, limb.get_variable());
            coefficient *= F::from(1_u64 << 16);
        }
        output_reconstruction = output_reconstruction + (scale, output_is_one.get_variable())
            - output_num.get_variable();
        cs.enforce(
            || "reconstruct output numerator",
            |lc| lc,
            |lc| lc,
            |_| output_reconstruction,
        );

        let remainder_scale_bits =
            (u64::BITS - self.r_den.saturating_sub(1).leading_zeros()) as usize;
        let (scale_floor, scale_floor_bits) = allocate_ranged(
            cs.namespace(|| "scale_floor"),
            "scale_floor_value",
            self.witness.scale_floor,
            16 * K + 1,
        )?;
        let (scale_remainder, _) = allocate_ranged(
            cs.namespace(|| "scale_remainder"),
            "scale_remainder_value",
            self.witness.remainder_scale as u128,
            remainder_scale_bits,
        )?;
        cs.enforce(
            || "R * quotient = D * scale_floor + scale remainder",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (F::from(self.r_num), quotient.get_variable())
                    - (F::from(self.r_den), scale_floor.get_variable())
                    - scale_remainder.get_variable()
            },
        );
        let scale_round_up = allocate_boolean_num(
            cs.namespace(|| "scale_round_up"),
            "scale_round_up_value",
            self.witness.scale_rounded_up,
        )?;
        enforce_small_remainder_rounding(
            cs.namespace(|| "scale_rounding_rule"),
            self.rounding,
            SmallRemainderRounding {
                floor_witness: self.witness.scale_floor,
                floor_bits: &scale_floor_bits,
                remainder: &scale_remainder,
                remainder_witness: self.witness.remainder_scale,
                divisor: self.r_den,
                round_up: &scale_round_up,
            },
        )?;
        cs.enforce(
            || "output = scale_floor + scale_round_up",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + scale_floor.get_variable() + scale_round_up.get_variable()
                    - output_num.get_variable()
            },
        );

        let output_witness = if self.witness.output_is_one {
            scale_u128
        } else {
            self.witness
                .output_limbs
                .iter()
                .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
        };
        let decision = enforce_threshold_decision(
            cs.namespace(|| "fixed_decision"),
            "fixed_decision",
            &output_num,
            output_witness,
            self.threshold_numerator,
            16 * K + 1,
        )?;

        let next_iteration = AllocatedNum::alloc(cs.namespace(|| "next_iteration"), || {
            z[0].get_value()
                .map(|value| value + F::ONE)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "increment iteration",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );

        let mut output = Vec::with_capacity(Self::arity_value());
        output.push(next_iteration);
        output.extend(output_limbs);
        output.push(output_is_one);
        output.push(decision);
        Ok(output)
    }
}

impl<F: PrimeField, const K: usize> StepCircuit<F> for CbdLogisticStepCircuit<K> {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert!(K > 0 && K <= 4);
        assert!(self.r_den > 0);
        assert_eq!(z.len(), Self::arity_value());
        let fractional_bits = 16 * K;
        let scale_u128 = 1_u128 << fractional_bits;
        let scale = field_from_u128::<F>(scale_u128);
        let divisor_u128 = self.r_den as u128 * scale_u128;
        let divisor = field_from_u128::<F>(divisor_u128);

        for (index, limb) in z[1..=K].iter().enumerate() {
            range_check(
                cs.namespace(|| format!("cbd_input_limb_{index}_range")),
                limb,
                self.witness.input_limbs[index] as u128,
                16,
            )?;
        }
        let input_is_one = &z[K + 1];
        enforce_boolean_num(cs.namespace(|| "cbd_input_is_one_boolean"), input_is_one);
        for (index, limb) in z[1..=K].iter().enumerate() {
            cs.enforce(
                || format!("cbd_canonical_input_one_limb_{index}"),
                |lc| lc + input_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }

        let input_num = AllocatedNum::alloc(cs.namespace(|| "cbd_input_numerator"), || {
            z[1..=K]
                .iter()
                .try_fold(F::ZERO, |acc, limb| {
                    limb.get_value()
                        .map(|value| acc * F::from(1_u64 << 16) + value)
                })
                .and_then(|value| {
                    input_is_one
                        .get_value()
                        .map(|is_one| value + is_one * scale)
                })
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let mut input_reconstruction = LinearCombination::zero();
        let mut coefficient = F::ONE;
        for limb in z[1..=K].iter().rev() {
            input_reconstruction = input_reconstruction + (coefficient, limb.get_variable());
            coefficient *= F::from(1_u64 << 16);
        }
        input_reconstruction =
            input_reconstruction + (scale, input_is_one.get_variable()) - input_num.get_variable();
        cs.enforce(
            || "cbd reconstruct input numerator",
            |lc| lc,
            |lc| lc,
            |_| input_reconstruction,
        );

        let input_radius = &z[K + 2];
        let input_radius_bits = range_check(
            cs.namespace(|| "cbd_input_radius_range"),
            input_radius,
            self.witness.input_radius,
            fractional_bits + 1,
        )?;
        let radius_high = input_radius_bits
            .last()
            .ok_or_else(|| SynthesisError::Unsatisfiable("missing radius high bit".to_owned()))?;
        let radius_low_nonzero = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_input_radius_low_nonzero"),
            &input_radius_bits[..fractional_bits],
        )?;
        cs.enforce(
            || "cbd radius at scale has zero low bits",
            |lc| lc + radius_high.get_variable(),
            |lc| lc + radius_low_nonzero.get_variable(),
            |lc| lc,
        );

        let complement = AllocatedNum::alloc(cs.namespace(|| "cbd_complement"), || {
            input_num
                .get_value()
                .map(|value| scale - value)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "cbd complement = S - input",
            |lc| lc,
            |lc| lc,
            |lc| lc + (scale, CS::one()) - input_num.get_variable() - complement.get_variable(),
        );
        let raw_product =
            input_num.mul(cs.namespace(|| "cbd_input_times_complement"), &complement)?;

        let (center_floor, center_floor_bits) = allocate_ranged(
            cs.namespace(|| "cbd_center_floor"),
            "cbd_center_floor_value",
            self.witness.center_floor,
            fractional_bits + 1,
        )?;
        let remainder_bits_count = bit_length_u128(divisor_u128 - 1);
        let (center_remainder, center_remainder_bits) = allocate_ranged(
            cs.namespace(|| "cbd_center_remainder"),
            "cbd_center_remainder_value",
            self.witness.center_remainder,
            remainder_bits_count,
        )?;
        let remainder_gap_witness = divisor_u128 - 1 - self.witness.center_remainder;
        let (remainder_gap, _) = allocate_ranged(
            cs.namespace(|| "cbd_center_remainder_gap"),
            "cbd_center_remainder_gap_value",
            remainder_gap_witness,
            remainder_bits_count,
        )?;
        cs.enforce(
            || "cbd center remainder is below divisor",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + center_remainder.get_variable() + remainder_gap.get_variable() + CS::one()
                    - (divisor, CS::one())
            },
        );
        cs.enforce(
            || "cbd fused center division",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (F::from(self.r_num), raw_product.get_variable())
                    - (divisor, center_floor.get_variable())
                    - center_remainder.get_variable()
            },
        );
        let center_round_up = allocate_boolean_num(
            cs.namespace(|| "cbd_center_round_up"),
            "cbd_center_round_up_value",
            self.witness.center_rounded_up,
        )?;
        enforce_constant_divisor_rounding(
            cs.namespace(|| "cbd_center_rounding_rule"),
            ConstantDivisorRoundingArgs {
                mode: CircuitRounding::NearestTiesToEven,
                floor_witness: self.witness.center_floor,
                remainder_witness: self.witness.center_remainder,
                divisor: divisor_u128,
                floor_bits: &center_floor_bits,
                remainder: &center_remainder,
                round_up: &center_round_up,
            },
        )?;
        let center_remainder_nonzero = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_center_remainder_nonzero"),
            &center_remainder_bits,
        )?;

        let mut output_limbs = Vec::with_capacity(K);
        for (index, &limb) in self.witness.output_limbs.iter().enumerate() {
            let (limb_num, _) = allocate_ranged(
                cs.namespace(|| format!("cbd_output_limb_{index}")),
                "cbd_output_limb_value",
                limb as u128,
                16,
            )?;
            output_limbs.push(limb_num);
        }
        let output_is_one = allocate_boolean_num(
            cs.namespace(|| "cbd_output_is_one"),
            "cbd_output_is_one_value",
            self.witness.output_is_one,
        )?;
        for (index, limb) in output_limbs.iter().enumerate() {
            cs.enforce(
                || format!("cbd_canonical_output_one_limb_{index}"),
                |lc| lc + output_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }
        let output_num = AllocatedNum::alloc(cs.namespace(|| "cbd_output_numerator"), || {
            output_limbs
                .iter()
                .try_fold(F::ZERO, |acc, limb| {
                    limb.get_value()
                        .map(|value| acc * F::from(1_u64 << 16) + value)
                })
                .and_then(|value| {
                    output_is_one
                        .get_value()
                        .map(|is_one| value + is_one * scale)
                })
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let mut output_reconstruction = LinearCombination::zero();
        let mut output_coefficient = F::ONE;
        for limb in output_limbs.iter().rev() {
            output_reconstruction =
                output_reconstruction + (output_coefficient, limb.get_variable());
            output_coefficient *= F::from(1_u64 << 16);
        }
        output_reconstruction = output_reconstruction + (scale, output_is_one.get_variable())
            - output_num.get_variable();
        cs.enforce(
            || "cbd reconstruct output numerator",
            |lc| lc,
            |lc| lc,
            |_| output_reconstruction,
        );
        cs.enforce(
            || "cbd output = center floor + round up",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + center_floor.get_variable() + center_round_up.get_variable()
                    - output_num.get_variable()
            },
        );

        let local_error_products = match self.radius_bound {
            CbdRadiusBound::Global => None,
            CbdRadiusBound::LocalTaylor => {
                let above_half = allocate_boolean_num(
                    cs.namespace(|| "cbd_center_above_half"),
                    "cbd_center_above_half_value",
                    self.witness.center_above_half,
                )?;
                let (center_distance, _) = allocate_ranged(
                    cs.namespace(|| "cbd_center_distance"),
                    "cbd_center_distance_value",
                    self.witness.center_distance,
                    fractional_bits + 1,
                )?;
                cs.enforce(
                    || "cbd distance above half",
                    |lc| lc + above_half.get_variable(),
                    |lc| {
                        lc + center_distance.get_variable() - (F::from(2), input_num.get_variable())
                            + (scale, CS::one())
                    },
                    |lc| lc,
                );
                cs.enforce(
                    || "cbd distance below half",
                    |lc| lc + CS::one() - above_half.get_variable(),
                    |lc| {
                        lc + center_distance.get_variable() - (scale, CS::one())
                            + (F::from(2), input_num.get_variable())
                    },
                    |lc| lc,
                );
                let radius_times_distance = input_radius.mul(
                    cs.namespace(|| "cbd_radius_times_center_distance"),
                    &center_distance,
                )?;
                let radius_squared =
                    input_radius.mul(cs.namespace(|| "cbd_radius_squared"), input_radius)?;
                Some((radius_times_distance, radius_squared))
            }
        };

        let radius_divisor_u128 = match self.radius_bound {
            CbdRadiusBound::Global => self.r_den as u128,
            CbdRadiusBound::LocalTaylor => self.r_den as u128 * scale_u128,
        };
        let radius_divisor = field_from_u128::<F>(radius_divisor_u128);
        let radius_floor_bit_count =
            fractional_bits + bit_length_u128(self.r_num.max(1) as u128) + 2;
        let (radius_floor, _) = allocate_ranged(
            cs.namespace(|| "cbd_radius_floor"),
            "cbd_radius_floor_value",
            self.witness.radius_floor,
            radius_floor_bit_count,
        )?;
        let radius_remainder_bits_count = bit_length_u128(radius_divisor_u128 - 1);
        let (radius_remainder, radius_remainder_bits) = allocate_ranged(
            cs.namespace(|| "cbd_radius_remainder"),
            "cbd_radius_remainder_value",
            self.witness.radius_remainder,
            radius_remainder_bits_count,
        )?;
        let radius_remainder_gap_witness = radius_divisor_u128 - 1 - self.witness.radius_remainder;
        let (radius_remainder_gap, _) = allocate_ranged(
            cs.namespace(|| "cbd_radius_remainder_gap"),
            "cbd_radius_remainder_gap_value",
            radius_remainder_gap_witness,
            radius_remainder_bits_count,
        )?;
        cs.enforce(
            || "cbd radius remainder is below divisor",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + radius_remainder.get_variable()
                    + radius_remainder_gap.get_variable()
                    + CS::one()
                    - (radius_divisor, CS::one())
            },
        );
        match &local_error_products {
            None => cs.enforce(
                || "cbd global radius division",
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + (F::from(self.r_num), input_radius.get_variable())
                        - (radius_divisor, radius_floor.get_variable())
                        - radius_remainder.get_variable()
                },
            ),
            Some((radius_times_distance, radius_squared)) => cs.enforce(
                || "cbd local radius division",
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + (F::from(self.r_num), radius_times_distance.get_variable())
                        + (F::from(self.r_num), radius_squared.get_variable())
                        - (radius_divisor, radius_floor.get_variable())
                        - radius_remainder.get_variable()
                },
            ),
        }
        let radius_remainder_nonzero = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_radius_remainder_nonzero"),
            &radius_remainder_bits,
        )?;

        let uncapped_bits_count = radius_floor_bit_count + 1;
        let (uncapped_radius, uncapped_radius_bits) = allocate_ranged(
            cs.namespace(|| "cbd_uncapped_radius"),
            "cbd_uncapped_radius_value",
            self.witness.uncapped_radius,
            uncapped_bits_count,
        )?;
        cs.enforce(
            || "cbd uncapped radius recurrence",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + radius_floor.get_variable()
                    + radius_remainder_nonzero.get_variable()
                    + center_remainder_nonzero.get_variable()
                    - uncapped_radius.get_variable()
            },
        );
        let radius_capped = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_radius_capped"),
            &uncapped_radius_bits[fractional_bits..],
        )?;
        debug_assert_eq!(
            self.witness.radius_capped,
            self.witness.uncapped_radius >= scale_u128
        );
        let (output_radius, _) = allocate_ranged(
            cs.namespace(|| "cbd_output_radius"),
            "cbd_output_radius_value",
            self.witness.output_radius,
            fractional_bits + 1,
        )?;
        cs.enforce(
            || "cbd capped radius equals scale",
            |lc| lc + radius_capped.get_variable(),
            |lc| lc + output_radius.get_variable() - (scale, CS::one()),
            |lc| lc,
        );
        cs.enforce(
            || "cbd uncapped radius is preserved",
            |lc| lc + CS::one() - radius_capped.get_variable(),
            |lc| lc + output_radius.get_variable() - uncapped_radius.get_variable(),
            |lc| lc,
        );

        let output_center_witness = if self.witness.output_is_one {
            scale_u128
        } else {
            self.witness
                .output_limbs
                .iter()
                .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
        };
        assert!(self.threshold_numerator > 0 && self.threshold_numerator <= scale_u128);
        let half_u128 = self.threshold_numerator;
        let decision_true_witness = self.witness.decision_half == 1;
        let decision_false_witness = self.witness.decision_half == 0;
        let decision_indeterminate_witness = self.witness.decision_half == 2;
        let decision_true = allocate_boolean_num(
            cs.namespace(|| "cbd_decision_true"),
            "cbd_decision_true_value",
            decision_true_witness,
        )?;
        let decision_false = allocate_boolean_num(
            cs.namespace(|| "cbd_decision_false"),
            "cbd_decision_false_value",
            decision_false_witness,
        )?;
        let decision_indeterminate = allocate_boolean_num(
            cs.namespace(|| "cbd_decision_indeterminate"),
            "cbd_decision_indeterminate_value",
            decision_indeterminate_witness,
        )?;
        cs.enforce(
            || "cbd decision one-hot",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + decision_true.get_variable()
                    + decision_false.get_variable()
                    + decision_indeterminate.get_variable()
                    - CS::one()
            },
        );
        let comparison_bits = fractional_bits + 2;
        let true_diff_witness = if decision_true_witness {
            output_center_witness
                .saturating_sub(self.witness.output_radius)
                .saturating_sub(half_u128)
        } else {
            0
        };
        let not_true_diff_witness = if !decision_true_witness {
            (half_u128 + self.witness.output_radius)
                .saturating_sub(1)
                .saturating_sub(output_center_witness)
        } else {
            0
        };
        let false_diff_witness = if decision_false_witness {
            half_u128
                .saturating_sub(1)
                .saturating_sub(output_center_witness)
                .saturating_sub(self.witness.output_radius)
        } else {
            0
        };
        let not_false_diff_witness = if !decision_false_witness {
            (output_center_witness + self.witness.output_radius).saturating_sub(half_u128)
        } else {
            0
        };
        let (true_diff, _) = allocate_ranged(
            cs.namespace(|| "cbd_true_diff"),
            "cbd_true_diff_value",
            true_diff_witness,
            comparison_bits,
        )?;
        let (not_true_diff, _) = allocate_ranged(
            cs.namespace(|| "cbd_not_true_diff"),
            "cbd_not_true_diff_value",
            not_true_diff_witness,
            comparison_bits,
        )?;
        let (false_diff, _) = allocate_ranged(
            cs.namespace(|| "cbd_false_diff"),
            "cbd_false_diff_value",
            false_diff_witness,
            comparison_bits,
        )?;
        let (not_false_diff, _) = allocate_ranged(
            cs.namespace(|| "cbd_not_false_diff"),
            "cbd_not_false_diff_value",
            not_false_diff_witness,
            comparison_bits,
        )?;
        let half = field_from_u128::<F>(half_u128);
        cs.enforce(
            || "cbd true proves lower >= half",
            |lc| lc + decision_true.get_variable(),
            |lc| {
                lc + output_num.get_variable()
                    - output_radius.get_variable()
                    - (half, CS::one())
                    - true_diff.get_variable()
            },
            |lc| lc,
        );
        cs.enforce(
            || "cbd not true proves lower < half",
            |lc| lc + CS::one() - decision_true.get_variable(),
            |lc| {
                lc + (half, CS::one()) + output_radius.get_variable()
                    - CS::one()
                    - output_num.get_variable()
                    - not_true_diff.get_variable()
            },
            |lc| lc,
        );
        cs.enforce(
            || "cbd false proves upper < half",
            |lc| lc + decision_false.get_variable(),
            |lc| {
                lc + (half, CS::one())
                    - CS::one()
                    - output_num.get_variable()
                    - output_radius.get_variable()
                    - false_diff.get_variable()
            },
            |lc| lc,
        );
        cs.enforce(
            || "cbd not false proves upper >= half",
            |lc| lc + CS::one() - decision_false.get_variable(),
            |lc| {
                lc + output_num.get_variable() + output_radius.get_variable()
                    - (half, CS::one())
                    - not_false_diff.get_variable()
            },
            |lc| lc,
        );
        let (decision_code, _) = allocate_ranged(
            cs.namespace(|| "cbd_decision_code"),
            "cbd_decision_code_value",
            self.witness.decision_half as u128,
            2,
        )?;
        cs.enforce(
            || "cbd decision code",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + decision_true.get_variable()
                    + (F::from(2), decision_indeterminate.get_variable())
                    - decision_code.get_variable()
            },
        );

        let next_iteration = AllocatedNum::alloc(cs.namespace(|| "cbd_next_iteration"), || {
            z[0].get_value()
                .map(|value| value + F::ONE)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "cbd increment iteration",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );

        let mut output = Vec::with_capacity(Self::arity_value());
        output.push(next_iteration);
        output.extend(output_limbs);
        output.push(output_is_one);
        output.push(output_radius);
        output.push(decision_code);
        Ok(output)
    }
}

impl<F: PrimeField, const K: usize> StepCircuit<F> for CbdSequentialLogisticStepCircuit<K> {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert!(K > 0 && K <= 4);
        assert!(self.r_den > 0);
        assert_eq!(z.len(), Self::arity_value());
        let fractional_bits = 16 * K;
        let scale_u128 = 1_u128 << fractional_bits;
        let scale = field_from_u128::<F>(scale_u128);

        for (index, limb) in z[1..=K].iter().enumerate() {
            range_check(
                cs.namespace(|| format!("cbd_seq_input_limb_{index}_range")),
                limb,
                self.witness.input_limbs[index] as u128,
                16,
            )?;
        }
        let input_is_one = &z[K + 1];
        enforce_boolean_num(
            cs.namespace(|| "cbd_seq_input_is_one_boolean"),
            input_is_one,
        );
        for (index, limb) in z[1..=K].iter().enumerate() {
            cs.enforce(
                || format!("cbd_seq_canonical_input_one_limb_{index}"),
                |lc| lc + input_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }
        let input_num = AllocatedNum::alloc(cs.namespace(|| "cbd_seq_input_numerator"), || {
            z[1..=K]
                .iter()
                .try_fold(F::ZERO, |acc, limb| {
                    limb.get_value()
                        .map(|value| acc * F::from(1_u64 << 16) + value)
                })
                .and_then(|value| {
                    input_is_one
                        .get_value()
                        .map(|is_one| value + is_one * scale)
                })
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let mut input_reconstruction = LinearCombination::zero();
        let mut coefficient = F::ONE;
        for limb in z[1..=K].iter().rev() {
            input_reconstruction = input_reconstruction + (coefficient, limb.get_variable());
            coefficient *= F::from(1_u64 << 16);
        }
        input_reconstruction =
            input_reconstruction + (scale, input_is_one.get_variable()) - input_num.get_variable();
        cs.enforce(
            || "cbd_seq reconstruct input",
            |lc| lc,
            |lc| lc,
            |_| input_reconstruction,
        );

        let input_radius = &z[K + 2];
        let input_radius_bits = range_check(
            cs.namespace(|| "cbd_seq_input_radius_range"),
            input_radius,
            self.witness.input_radius,
            fractional_bits + 1,
        )?;
        let radius_high = input_radius_bits
            .last()
            .ok_or_else(|| SynthesisError::Unsatisfiable("missing radius high bit".to_owned()))?;
        let radius_low_nonzero = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_seq_input_radius_low_nonzero"),
            &input_radius_bits[..fractional_bits],
        )?;
        cs.enforce(
            || "cbd_seq radius at scale has zero low bits",
            |lc| lc + radius_high.get_variable(),
            |lc| lc + radius_low_nonzero.get_variable(),
            |lc| lc,
        );

        let complement = AllocatedNum::alloc(cs.namespace(|| "cbd_seq_complement"), || {
            input_num
                .get_value()
                .map(|value| scale - value)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "cbd_seq complement",
            |lc| lc,
            |lc| lc,
            |lc| lc + (scale, CS::one()) - input_num.get_variable() - complement.get_variable(),
        );
        let raw_product = input_num.mul(
            cs.namespace(|| "cbd_seq_input_times_complement"),
            &complement,
        )?;

        let above_half = allocate_boolean_num(
            cs.namespace(|| "cbd_seq_center_above_half"),
            "cbd_seq_center_above_half_value",
            self.witness.center_above_half,
        )?;
        let (center_distance, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_center_distance"),
            "cbd_seq_center_distance_value",
            self.witness.center_distance,
            fractional_bits + 1,
        )?;
        cs.enforce(
            || "cbd_seq distance above half",
            |lc| lc + above_half.get_variable(),
            |lc| {
                lc + center_distance.get_variable() - (F::from(2), input_num.get_variable())
                    + (scale, CS::one())
            },
            |lc| lc,
        );
        cs.enforce(
            || "cbd_seq distance below half",
            |lc| lc + CS::one() - above_half.get_variable(),
            |lc| {
                lc + center_distance.get_variable() - (scale, CS::one())
                    + (F::from(2), input_num.get_variable())
            },
            |lc| lc,
        );

        let (product_floor, product_floor_bits) = allocate_ranged(
            cs.namespace(|| "cbd_seq_product_floor"),
            "cbd_seq_product_floor_value",
            self.witness.product_floor,
            fractional_bits + 1,
        )?;
        let (product_remainder, product_remainder_bits) = allocate_ranged(
            cs.namespace(|| "cbd_seq_product_remainder"),
            "cbd_seq_product_remainder_value",
            self.witness.product_remainder,
            fractional_bits,
        )?;
        cs.enforce(
            || "cbd_seq product division",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + raw_product.get_variable()
                    - (scale, product_floor.get_variable())
                    - product_remainder.get_variable()
            },
        );
        let product_round_up = allocate_boolean_num(
            cs.namespace(|| "cbd_seq_product_round_up"),
            "cbd_seq_product_round_up_value",
            self.witness.product_rounded_up,
        )?;
        enforce_rounding_power_of_two(
            cs.namespace(|| "cbd_seq_product_rounding"),
            CircuitRounding::NearestTiesToEven,
            self.witness.product_floor,
            self.witness.product_remainder,
            &product_floor_bits,
            &product_remainder_bits,
            &product_round_up,
        )?;
        let product_num = AllocatedNum::alloc(cs.namespace(|| "cbd_seq_product"), || {
            Ok(field_from_u128(
                self.witness.product_floor + u128::from(self.witness.product_rounded_up),
            ))
        })?;
        cs.enforce(
            || "cbd_seq rounded product",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + product_floor.get_variable() + product_round_up.get_variable()
                    - product_num.get_variable()
            },
        );
        let product_remainder_nonzero = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_seq_product_remainder_nonzero"),
            &product_remainder_bits,
        )?;

        let mut product_limbs = Vec::with_capacity(K);
        for (index, limb) in self.witness.product_limbs.iter().enumerate() {
            let (limb_num, _) = allocate_ranged(
                cs.namespace(|| format!("cbd_seq_product_limb_{index}")),
                "cbd_seq_product_limb_value",
                *limb as u128,
                16,
            )?;
            product_limbs.push(limb_num);
        }
        let product_is_one = allocate_boolean_num(
            cs.namespace(|| "cbd_seq_product_is_one"),
            "cbd_seq_product_is_one_value",
            self.witness.product_is_one,
        )?;
        for (index, limb) in product_limbs.iter().enumerate() {
            cs.enforce(
                || format!("cbd_seq_canonical_product_one_limb_{index}"),
                |lc| lc + product_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }
        let mut product_reconstruction = LinearCombination::zero();
        let mut product_coefficient = F::ONE;
        for limb in product_limbs.iter().rev() {
            product_reconstruction =
                product_reconstruction + (product_coefficient, limb.get_variable());
            product_coefficient *= F::from(1_u64 << 16);
        }
        product_reconstruction = product_reconstruction + (scale, product_is_one.get_variable())
            - product_num.get_variable();
        cs.enforce(
            || "cbd_seq reconstruct product",
            |lc| lc,
            |lc| lc,
            |_| product_reconstruction,
        );

        let radius_times_distance = input_radius.mul(
            cs.namespace(|| "cbd_seq_radius_times_distance"),
            &center_distance,
        )?;
        let radius_squared =
            input_radius.mul(cs.namespace(|| "cbd_seq_radius_squared"), input_radius)?;
        let (product_radius_floor, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_product_radius_floor"),
            "cbd_seq_product_radius_floor_value",
            self.witness.product_radius_floor,
            fractional_bits + 2,
        )?;
        let (product_radius_remainder, product_radius_remainder_bits) = allocate_ranged(
            cs.namespace(|| "cbd_seq_product_radius_remainder"),
            "cbd_seq_product_radius_remainder_value",
            self.witness.product_radius_remainder,
            fractional_bits,
        )?;
        cs.enforce(
            || "cbd_seq product radius division",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + radius_times_distance.get_variable() + radius_squared.get_variable()
                    - (scale, product_radius_floor.get_variable())
                    - product_radius_remainder.get_variable()
            },
        );
        let product_radius_remainder_nonzero = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_seq_product_radius_remainder_nonzero"),
            &product_radius_remainder_bits,
        )?;
        let product_uncapped_bits_count = fractional_bits + 3;
        let (product_uncapped_radius, product_uncapped_radius_bits) = allocate_ranged(
            cs.namespace(|| "cbd_seq_product_uncapped_radius"),
            "cbd_seq_product_uncapped_radius_value",
            self.witness.product_uncapped_radius,
            product_uncapped_bits_count,
        )?;
        cs.enforce(
            || "cbd_seq product radius recurrence",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + product_radius_floor.get_variable()
                    + product_radius_remainder_nonzero.get_variable()
                    + product_remainder_nonzero.get_variable()
                    - product_uncapped_radius.get_variable()
            },
        );
        let product_radius_capped = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_seq_product_radius_capped"),
            &product_uncapped_radius_bits[fractional_bits..],
        )?;
        let (product_radius, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_product_radius"),
            "cbd_seq_product_radius_value",
            self.witness.product_radius,
            fractional_bits + 1,
        )?;
        cs.enforce(
            || "cbd_seq capped product radius",
            |lc| lc + product_radius_capped.get_variable(),
            |lc| lc + product_radius.get_variable() - (scale, CS::one()),
            |lc| lc,
        );
        cs.enforce(
            || "cbd_seq uncapped product radius",
            |lc| lc + CS::one() - product_radius_capped.get_variable(),
            |lc| lc + product_radius.get_variable() - product_uncapped_radius.get_variable(),
            |lc| lc,
        );

        let (output_floor, output_floor_bits) = allocate_ranged(
            cs.namespace(|| "cbd_seq_output_floor"),
            "cbd_seq_output_floor_value",
            self.witness.output_floor,
            fractional_bits + 1,
        )?;
        let output_remainder_bits_count = bit_length_u128(self.r_den.saturating_sub(1) as u128);
        let (output_remainder, output_remainder_bits) = allocate_ranged(
            cs.namespace(|| "cbd_seq_output_remainder"),
            "cbd_seq_output_remainder_value",
            self.witness.output_remainder,
            output_remainder_bits_count,
        )?;
        let output_remainder_gap_witness = self.r_den as u128 - 1 - self.witness.output_remainder;
        let (output_remainder_gap, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_output_remainder_gap"),
            "cbd_seq_output_remainder_gap_value",
            output_remainder_gap_witness,
            output_remainder_bits_count,
        )?;
        cs.enforce(
            || "cbd_seq output remainder below divisor",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + output_remainder.get_variable()
                    + output_remainder_gap.get_variable()
                    + CS::one()
                    - (F::from(self.r_den), CS::one())
            },
        );
        cs.enforce(
            || "cbd_seq scale product center",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (F::from(self.r_num), product_num.get_variable())
                    - (F::from(self.r_den), output_floor.get_variable())
                    - output_remainder.get_variable()
            },
        );
        let output_round_up = allocate_boolean_num(
            cs.namespace(|| "cbd_seq_output_round_up"),
            "cbd_seq_output_round_up_value",
            self.witness.output_rounded_up,
        )?;
        enforce_constant_divisor_rounding(
            cs.namespace(|| "cbd_seq_output_rounding"),
            ConstantDivisorRoundingArgs {
                mode: CircuitRounding::NearestTiesToEven,
                floor_witness: self.witness.output_floor,
                remainder_witness: self.witness.output_remainder,
                divisor: self.r_den as u128,
                floor_bits: &output_floor_bits,
                remainder: &output_remainder,
                round_up: &output_round_up,
            },
        )?;
        let output_remainder_nonzero = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_seq_output_remainder_nonzero"),
            &output_remainder_bits,
        )?;

        let mut output_limbs = Vec::with_capacity(K);
        for (index, limb) in self.witness.output_limbs.iter().enumerate() {
            let (limb_num, _) = allocate_ranged(
                cs.namespace(|| format!("cbd_seq_output_limb_{index}")),
                "cbd_seq_output_limb_value",
                *limb as u128,
                16,
            )?;
            output_limbs.push(limb_num);
        }
        let output_is_one = allocate_boolean_num(
            cs.namespace(|| "cbd_seq_output_is_one"),
            "cbd_seq_output_is_one_value",
            self.witness.output_is_one,
        )?;
        for (index, limb) in output_limbs.iter().enumerate() {
            cs.enforce(
                || format!("cbd_seq_canonical_output_one_limb_{index}"),
                |lc| lc + output_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }
        let output_num = AllocatedNum::alloc(cs.namespace(|| "cbd_seq_output"), || {
            Ok(field_from_u128(
                self.witness.output_floor + u128::from(self.witness.output_rounded_up),
            ))
        })?;
        let mut output_reconstruction = LinearCombination::zero();
        let mut output_coefficient = F::ONE;
        for limb in output_limbs.iter().rev() {
            output_reconstruction =
                output_reconstruction + (output_coefficient, limb.get_variable());
            output_coefficient *= F::from(1_u64 << 16);
        }
        output_reconstruction = output_reconstruction + (scale, output_is_one.get_variable())
            - output_num.get_variable();
        cs.enforce(
            || "cbd_seq reconstruct output",
            |lc| lc,
            |lc| lc,
            |_| output_reconstruction,
        );
        cs.enforce(
            || "cbd_seq rounded output",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + output_floor.get_variable() + output_round_up.get_variable()
                    - output_num.get_variable()
            },
        );

        let output_radius_floor_bits =
            fractional_bits + bit_length_u128(self.r_num.max(1) as u128) + 2;
        let (output_radius_floor, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_output_radius_floor"),
            "cbd_seq_output_radius_floor_value",
            self.witness.output_radius_floor,
            output_radius_floor_bits,
        )?;
        let (output_radius_remainder, output_radius_remainder_bits) = allocate_ranged(
            cs.namespace(|| "cbd_seq_output_radius_remainder"),
            "cbd_seq_output_radius_remainder_value",
            self.witness.output_radius_remainder,
            output_remainder_bits_count,
        )?;
        let output_radius_gap_witness =
            self.r_den as u128 - 1 - self.witness.output_radius_remainder;
        let (output_radius_gap, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_output_radius_gap"),
            "cbd_seq_output_radius_gap_value",
            output_radius_gap_witness,
            output_remainder_bits_count,
        )?;
        cs.enforce(
            || "cbd_seq output radius remainder below divisor",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + output_radius_remainder.get_variable()
                    + output_radius_gap.get_variable()
                    + CS::one()
                    - (F::from(self.r_den), CS::one())
            },
        );
        cs.enforce(
            || "cbd_seq output radius division",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (F::from(self.r_num), product_radius.get_variable())
                    - (F::from(self.r_den), output_radius_floor.get_variable())
                    - output_radius_remainder.get_variable()
            },
        );
        let output_radius_remainder_nonzero = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_seq_output_radius_remainder_nonzero"),
            &output_radius_remainder_bits,
        )?;
        let output_uncapped_bits_count = output_radius_floor_bits + 1;
        let (output_uncapped_radius, output_uncapped_radius_bits) = allocate_ranged(
            cs.namespace(|| "cbd_seq_output_uncapped_radius"),
            "cbd_seq_output_uncapped_radius_value",
            self.witness.output_uncapped_radius,
            output_uncapped_bits_count,
        )?;
        cs.enforce(
            || "cbd_seq output radius recurrence",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + output_radius_floor.get_variable()
                    + output_radius_remainder_nonzero.get_variable()
                    + output_remainder_nonzero.get_variable()
                    - output_uncapped_radius.get_variable()
            },
        );
        let output_radius_capped = allocate_nonzero_indicator_from_bits(
            cs.namespace(|| "cbd_seq_output_radius_capped"),
            &output_uncapped_radius_bits[fractional_bits..],
        )?;
        let (output_radius, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_output_radius"),
            "cbd_seq_output_radius_value",
            self.witness.output_radius,
            fractional_bits + 1,
        )?;
        cs.enforce(
            || "cbd_seq capped output radius",
            |lc| lc + output_radius_capped.get_variable(),
            |lc| lc + output_radius.get_variable() - (scale, CS::one()),
            |lc| lc,
        );
        cs.enforce(
            || "cbd_seq uncapped output radius",
            |lc| lc + CS::one() - output_radius_capped.get_variable(),
            |lc| lc + output_radius.get_variable() - output_uncapped_radius.get_variable(),
            |lc| lc,
        );

        let output_center_witness = if self.witness.output_is_one {
            scale_u128
        } else {
            self.witness
                .output_limbs
                .iter()
                .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
        };
        assert!(self.threshold_numerator > 0 && self.threshold_numerator <= scale_u128);
        let half_u128 = self.threshold_numerator;
        let decision_true_witness = self.witness.decision_half == 1;
        let decision_false_witness = self.witness.decision_half == 0;
        let decision_indeterminate_witness = self.witness.decision_half == 2;
        let decision_true = allocate_boolean_num(
            cs.namespace(|| "cbd_seq_decision_true"),
            "cbd_seq_decision_true_value",
            decision_true_witness,
        )?;
        let decision_false = allocate_boolean_num(
            cs.namespace(|| "cbd_seq_decision_false"),
            "cbd_seq_decision_false_value",
            decision_false_witness,
        )?;
        let decision_indeterminate = allocate_boolean_num(
            cs.namespace(|| "cbd_seq_decision_indeterminate"),
            "cbd_seq_decision_indeterminate_value",
            decision_indeterminate_witness,
        )?;
        cs.enforce(
            || "cbd_seq decision one-hot",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + decision_true.get_variable()
                    + decision_false.get_variable()
                    + decision_indeterminate.get_variable()
                    - CS::one()
            },
        );
        let comparison_bits = fractional_bits + 2;
        let true_diff_witness = if decision_true_witness {
            output_center_witness
                .saturating_sub(self.witness.output_radius)
                .saturating_sub(half_u128)
        } else {
            0
        };
        let not_true_diff_witness = if !decision_true_witness {
            (half_u128 + self.witness.output_radius)
                .saturating_sub(1)
                .saturating_sub(output_center_witness)
        } else {
            0
        };
        let false_diff_witness = if decision_false_witness {
            half_u128
                .saturating_sub(1)
                .saturating_sub(output_center_witness)
                .saturating_sub(self.witness.output_radius)
        } else {
            0
        };
        let not_false_diff_witness = if !decision_false_witness {
            (output_center_witness + self.witness.output_radius).saturating_sub(half_u128)
        } else {
            0
        };
        let (true_diff, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_true_diff"),
            "cbd_seq_true_diff_value",
            true_diff_witness,
            comparison_bits,
        )?;
        let (not_true_diff, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_not_true_diff"),
            "cbd_seq_not_true_diff_value",
            not_true_diff_witness,
            comparison_bits,
        )?;
        let (false_diff, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_false_diff"),
            "cbd_seq_false_diff_value",
            false_diff_witness,
            comparison_bits,
        )?;
        let (not_false_diff, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_not_false_diff"),
            "cbd_seq_not_false_diff_value",
            not_false_diff_witness,
            comparison_bits,
        )?;
        let half = field_from_u128::<F>(half_u128);
        cs.enforce(
            || "cbd_seq true proves lower >= half",
            |lc| lc + decision_true.get_variable(),
            |lc| {
                lc + output_num.get_variable()
                    - output_radius.get_variable()
                    - (half, CS::one())
                    - true_diff.get_variable()
            },
            |lc| lc,
        );
        cs.enforce(
            || "cbd_seq not true proves lower < half",
            |lc| lc + CS::one() - decision_true.get_variable(),
            |lc| {
                lc + (half, CS::one()) + output_radius.get_variable()
                    - CS::one()
                    - output_num.get_variable()
                    - not_true_diff.get_variable()
            },
            |lc| lc,
        );
        cs.enforce(
            || "cbd_seq false proves upper < half",
            |lc| lc + decision_false.get_variable(),
            |lc| {
                lc + (half, CS::one())
                    - CS::one()
                    - output_num.get_variable()
                    - output_radius.get_variable()
                    - false_diff.get_variable()
            },
            |lc| lc,
        );
        cs.enforce(
            || "cbd_seq not false proves upper >= half",
            |lc| lc + CS::one() - decision_false.get_variable(),
            |lc| {
                lc + output_num.get_variable() + output_radius.get_variable()
                    - (half, CS::one())
                    - not_false_diff.get_variable()
            },
            |lc| lc,
        );
        let (decision_code, _) = allocate_ranged(
            cs.namespace(|| "cbd_seq_decision_code"),
            "cbd_seq_decision_code_value",
            self.witness.decision_half as u128,
            2,
        )?;
        cs.enforce(
            || "cbd_seq decision code",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + decision_true.get_variable()
                    + (F::from(2), decision_indeterminate.get_variable())
                    - decision_code.get_variable()
            },
        );

        let next_iteration =
            AllocatedNum::alloc(cs.namespace(|| "cbd_seq_next_iteration"), || {
                z[0].get_value()
                    .map(|value| value + F::ONE)
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;
        cs.enforce(
            || "cbd_seq increment iteration",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );

        let mut result = Vec::with_capacity(Self::arity_value());
        result.push(next_iteration);
        result.extend(output_limbs);
        result.push(output_is_one);
        result.push(output_radius);
        result.push(decision_code);
        Ok(result)
    }
}

impl<F: PrimeField, const K: usize> StepCircuit<F> for CarryLogisticStepCircuit<K> {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert!(K > 0 && K <= 4);
        assert_eq!(z.len(), Self::arity_value());
        let scale_bits = 16 * K;
        let scale_u128 = 1_u128 << scale_bits;
        let base = F::from(1_u64 << 16);
        let scale = field_from_u128::<F>(scale_u128);

        for (index, limb) in z[1..=K].iter().enumerate() {
            range_check(
                cs.namespace(|| format!("carry_input_limb_{index}_range")),
                limb,
                self.witness.input_limbs[index] as u128,
                16,
            )?;
        }
        let input_is_one = &z[K + 1];
        enforce_boolean_num(cs.namespace(|| "carry_input_is_one_boolean"), input_is_one);
        for (index, limb) in z[1..=K].iter().enumerate() {
            cs.enforce(
                || format!("carry_canonical_input_one_limb_{index}"),
                |lc| lc + input_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }

        let input_num = AllocatedNum::alloc(cs.namespace(|| "carry_input_numerator"), || {
            let mut value = F::ZERO;
            for limb in &z[1..=K] {
                value = value * base + limb.get_value().ok_or(SynthesisError::AssignmentMissing)?;
            }
            value += input_is_one
                .get_value()
                .ok_or(SynthesisError::AssignmentMissing)?
                * scale;
            Ok(value)
        })?;
        let mut input_reconstruction = LinearCombination::zero();
        let mut coefficient = F::ONE;
        for limb in z[1..=K].iter().rev() {
            input_reconstruction = input_reconstruction + (coefficient, limb.get_variable());
            coefficient *= base;
        }
        input_reconstruction =
            input_reconstruction + (scale, input_is_one.get_variable()) - input_num.get_variable();
        cs.enforce(
            || "carry_reconstruct_input_numerator",
            |lc| lc,
            |lc| lc,
            |_| input_reconstruction,
        );

        let mut complement_limbs = Vec::with_capacity(K);
        for (index, &limb) in self.carry_witness.complement_limbs.iter().enumerate() {
            let (limb, _) = allocate_ranged(
                cs.namespace(|| format!("carry_complement_limb_{index}")),
                "carry_complement_limb_value",
                limb as u128,
                16,
            )?;
            complement_limbs.push(limb);
        }
        let complement_is_one = allocate_boolean_num(
            cs.namespace(|| "carry_complement_is_one"),
            "carry_complement_is_one_value",
            self.carry_witness.complement_is_one,
        )?;
        for (index, limb) in complement_limbs.iter().enumerate() {
            cs.enforce(
                || format!("carry_canonical_complement_one_limb_{index}"),
                |lc| lc + complement_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }
        let complement_num =
            AllocatedNum::alloc(cs.namespace(|| "carry_complement_numerator"), || {
                let mut value = F::ZERO;
                for limb in &complement_limbs {
                    value =
                        value * base + limb.get_value().ok_or(SynthesisError::AssignmentMissing)?;
                }
                value += complement_is_one
                    .get_value()
                    .ok_or(SynthesisError::AssignmentMissing)?
                    * scale;
                Ok(value)
            })?;
        let mut complement_reconstruction = LinearCombination::zero();
        let mut coefficient = F::ONE;
        for limb in complement_limbs.iter().rev() {
            complement_reconstruction =
                complement_reconstruction + (coefficient, limb.get_variable());
            coefficient *= base;
        }
        complement_reconstruction = complement_reconstruction
            + (scale, complement_is_one.get_variable())
            - complement_num.get_variable();
        cs.enforce(
            || "carry_reconstruct_complement_numerator",
            |lc| lc,
            |lc| lc,
            |_| complement_reconstruction,
        );
        cs.enforce(
            || "carry_complement = S - input",
            |lc| lc,
            |lc| lc,
            |lc| lc + (scale, CS::one()) - input_num.get_variable() - complement_num.get_variable(),
        );

        let input_le = z[1..=K].iter().rev().collect::<Vec<_>>();
        let complement_le = complement_limbs.iter().rev().collect::<Vec<_>>();
        let mut pair_products = Vec::with_capacity(K * K);
        for (i, a) in input_le.iter().enumerate() {
            for (j, b) in complement_le.iter().enumerate() {
                pair_products.push(a.mul(cs.namespace(|| format!("carry_p_{i}_{j}")), b)?);
            }
        }

        let mut digits = Vec::with_capacity(2 * K);
        for (index, &digit) in self.carry_witness.product_digits_le.iter().enumerate() {
            let (digit, _) = allocate_ranged(
                cs.namespace(|| format!("carry_product_digit_{index}")),
                "carry_product_digit_value",
                digit as u128,
                16,
            )?;
            digits.push(digit);
        }
        let mut carries = Vec::with_capacity(2 * K - 1);
        for (index, &carry) in self.carry_witness.carries.iter().enumerate() {
            let (carry, _) = allocate_ranged(
                cs.namespace(|| format!("carry_value_{index}")),
                "carry_value",
                carry,
                20,
            )?;
            carries.push(carry);
        }

        for column in 0..(2 * K - 1) {
            cs.enforce(
                || format!("carry column {column}"),
                |lc| lc,
                |lc| lc,
                |mut lc| {
                    for i in 0..K {
                        if column >= i {
                            let j = column - i;
                            if j < K {
                                lc = lc + pair_products[i * K + j].get_variable();
                            }
                        }
                    }
                    if column > 0 {
                        lc = lc + carries[column - 1].get_variable();
                    }
                    lc - digits[column].get_variable() - (base, carries[column].get_variable())
                },
            );
        }
        cs.enforce(
            || "carry final column",
            |lc| lc,
            |lc| lc,
            |lc| lc + carries[2 * K - 2].get_variable() - digits[2 * K - 1].get_variable(),
        );

        let (product_floor, product_floor_bits) = allocate_ranged(
            cs.namespace(|| "carry_product_floor"),
            "carry_product_floor_value",
            self.witness.product_floor,
            scale_bits + 1,
        )?;
        let mut floor_reconstruction = LinearCombination::zero();
        let mut coefficient = F::ONE;
        for digit in digits[K..(2 * K)].iter() {
            floor_reconstruction = floor_reconstruction + (coefficient, digit.get_variable());
            coefficient *= base;
        }
        floor_reconstruction = floor_reconstruction - product_floor.get_variable();
        cs.enforce(
            || "carry product floor from high digits",
            |lc| lc,
            |lc| lc,
            |_| floor_reconstruction,
        );
        let (remainder, remainder_bits) = allocate_ranged(
            cs.namespace(|| "carry_product_remainder"),
            "carry_product_remainder_value",
            self.witness.remainder_product,
            scale_bits,
        )?;
        let mut remainder_reconstruction = LinearCombination::zero();
        let mut coefficient = F::ONE;
        for digit in digits[..K].iter() {
            remainder_reconstruction =
                remainder_reconstruction + (coefficient, digit.get_variable());
            coefficient *= base;
        }
        remainder_reconstruction = remainder_reconstruction - remainder.get_variable();
        cs.enforce(
            || "carry product remainder from low digits",
            |lc| lc,
            |lc| lc,
            |_| remainder_reconstruction,
        );

        let product_round_up = allocate_boolean_num(
            cs.namespace(|| "carry_product_round_up"),
            "carry_product_round_up_value",
            self.witness.product_rounded_up,
        )?;
        enforce_rounding_power_of_two(
            cs.namespace(|| "carry_product_rounding_rule"),
            self.rounding,
            self.witness.product_floor,
            self.witness.remainder_product,
            &product_floor_bits,
            &remainder_bits,
            &product_round_up,
        )?;
        let quotient = AllocatedNum::alloc(cs.namespace(|| "carry_product_quotient"), || {
            Ok(field_from_u128(self.witness.quotient_product))
        })?;
        cs.enforce(
            || "carry product quotient = floor + round_up",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + product_floor.get_variable() + product_round_up.get_variable()
                    - quotient.get_variable()
            },
        );

        let mut output_limbs = Vec::with_capacity(K);
        for (index, &limb) in self.witness.output_limbs.iter().enumerate() {
            let (limb, _) = allocate_ranged(
                cs.namespace(|| format!("carry_output_limb_{index}")),
                "carry_output_limb_value",
                limb as u128,
                16,
            )?;
            output_limbs.push(limb);
        }
        let output_is_one = allocate_boolean_num(
            cs.namespace(|| "carry_output_is_one"),
            "carry_output_is_one_value",
            self.witness.output_is_one,
        )?;
        for (index, limb) in output_limbs.iter().enumerate() {
            cs.enforce(
                || format!("carry_canonical_output_one_limb_{index}"),
                |lc| lc + output_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }
        let output_num = AllocatedNum::alloc(cs.namespace(|| "carry_output_numerator"), || {
            let mut value = F::ZERO;
            for limb in &output_limbs {
                value = value * base + limb.get_value().ok_or(SynthesisError::AssignmentMissing)?;
            }
            value += output_is_one
                .get_value()
                .ok_or(SynthesisError::AssignmentMissing)?
                * scale;
            Ok(value)
        })?;
        let mut output_reconstruction = LinearCombination::zero();
        let mut coefficient = F::ONE;
        for limb in output_limbs.iter().rev() {
            output_reconstruction = output_reconstruction + (coefficient, limb.get_variable());
            coefficient *= base;
        }
        output_reconstruction = output_reconstruction + (scale, output_is_one.get_variable())
            - output_num.get_variable();
        cs.enforce(
            || "carry_reconstruct_output_numerator",
            |lc| lc,
            |lc| lc,
            |_| output_reconstruction,
        );

        let remainder_scale_bits =
            (u64::BITS - self.r_den.saturating_sub(1).leading_zeros()) as usize;
        let (scale_floor, scale_floor_bits) = allocate_ranged(
            cs.namespace(|| "carry_scale_floor"),
            "carry_scale_floor_value",
            self.witness.scale_floor,
            scale_bits + 1,
        )?;
        let (scale_remainder, _) = allocate_ranged(
            cs.namespace(|| "carry_scale_remainder"),
            "carry_scale_remainder_value",
            self.witness.remainder_scale as u128,
            remainder_scale_bits,
        )?;
        cs.enforce(
            || "carry R * quotient = D * scale_floor + scale remainder",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (F::from(self.r_num), quotient.get_variable())
                    - (F::from(self.r_den), scale_floor.get_variable())
                    - scale_remainder.get_variable()
            },
        );
        let scale_round_up = allocate_boolean_num(
            cs.namespace(|| "carry_scale_round_up"),
            "carry_scale_round_up_value",
            self.witness.scale_rounded_up,
        )?;
        enforce_small_remainder_rounding(
            cs.namespace(|| "carry_scale_rounding_rule"),
            self.rounding,
            SmallRemainderRounding {
                floor_witness: self.witness.scale_floor,
                floor_bits: &scale_floor_bits,
                remainder: &scale_remainder,
                remainder_witness: self.witness.remainder_scale,
                divisor: self.r_den,
                round_up: &scale_round_up,
            },
        )?;
        cs.enforce(
            || "carry output = scale_floor + scale_round_up",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + scale_floor.get_variable() + scale_round_up.get_variable()
                    - output_num.get_variable()
            },
        );

        let output_witness = if self.witness.output_is_one {
            scale_u128
        } else {
            self.witness
                .output_limbs
                .iter()
                .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
        };
        let decision = enforce_threshold_decision(
            cs.namespace(|| "carry_decision"),
            "carry_decision",
            &output_num,
            output_witness,
            self.threshold_numerator,
            scale_bits + 1,
        )?;

        let next_iteration = AllocatedNum::alloc(cs.namespace(|| "carry_next_iteration"), || {
            z[0].get_value()
                .map(|value| value + F::ONE)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "carry_increment_iteration",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );

        let mut output = Vec::with_capacity(Self::arity_value());
        output.push(next_iteration);
        output.extend(output_limbs);
        output.push(output_is_one);
        output.push(decision);
        Ok(output)
    }
}

impl<F: PrimeField> StepCircuit<F> for CompensatedLogisticStepCircuit {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        assert_eq!(z.len(), Self::arity_value());
        let scale_u128 = 1_u128 << 32;
        let fine_scale_u128 = scale_u128 * scale_u128;
        let half_u128 = scale_u128 / 2;
        let scale = field_from_u128::<F>(scale_u128);
        let fine_scale = field_from_u128::<F>(fine_scale_u128);
        let half = field_from_u128::<F>(half_u128);

        for (index, limb) in z[1..=2].iter().enumerate() {
            range_check(
                cs.namespace(|| format!("comp_input_limb_{index}_range")),
                limb,
                self.witness.input_limbs[index] as u128,
                16,
            )?;
        }
        let input_is_one = &z[3];
        enforce_boolean_num(cs.namespace(|| "comp_input_is_one_boolean"), input_is_one);
        for (index, limb) in z[1..=2].iter().enumerate() {
            cs.enforce(
                || format!("comp_canonical_input_one_limb_{index}"),
                |lc| lc + input_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }
        range_check(
            cs.namespace(|| "comp_input_residual_offset_range"),
            &z[4],
            self.witness.input_residual_offset as u128,
            33,
        )?;

        let input_num = AllocatedNum::alloc(cs.namespace(|| "comp_input_numerator"), || {
            let value = z[1]
                .get_value()
                .zip(z[2].get_value())
                .zip(input_is_one.get_value())
                .map(|((hi, lo), is_one)| hi * F::from(1_u64 << 16) + lo + is_one * scale);
            value.ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "comp_reconstruct_input_numerator",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (F::from(1_u64 << 16), z[1].get_variable())
                    + z[2].get_variable()
                    + (scale, input_is_one.get_variable())
                    - input_num.get_variable()
            },
        );

        let input_fine_value = {
            let input_num = if self.witness.input_is_one {
                scale_u128
            } else {
                ((self.witness.input_limbs[0] as u128) << 16) + self.witness.input_limbs[1] as u128
            };
            let residual = self.witness.input_residual_offset as i128 - half_u128 as i128;
            (input_num * scale_u128) as i128 + residual
        };
        if !(0..=fine_scale_u128 as i128).contains(&input_fine_value) {
            return Err(SynthesisError::Unsatisfiable(
                "compensated input fine numerator out of range".to_owned(),
            ));
        }
        let (input_fine, _) = allocate_ranged(
            cs.namespace(|| "comp_input_fine"),
            "comp_input_fine_value",
            input_fine_value as u128,
            65,
        )?;
        cs.enforce(
            || "comp_input_fine = S*V + offset - half",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (scale, input_num.get_variable()) + z[4].get_variable()
                    - (half, CS::one())
                    - input_fine.get_variable()
            },
        );

        let complement = AllocatedNum::alloc(cs.namespace(|| "comp_fine_complement"), || {
            input_fine
                .get_value()
                .map(|value| fine_scale - value)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        range_check(
            cs.namespace(|| "comp_fine_complement_range"),
            &complement,
            fine_scale_u128 - input_fine_value as u128,
            65,
        )?;
        cs.enforce(
            || "comp_complement = S2 - input_fine",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (fine_scale, CS::one()) - input_fine.get_variable() - complement.get_variable()
            },
        );
        let product =
            input_fine.mul(cs.namespace(|| "comp_input_times_complement"), &complement)?;

        let divisor_u128 = self.r_den as u128 * fine_scale_u128;
        let (fine_floor, fine_floor_bits) = allocate_ranged(
            cs.namespace(|| "comp_fine_floor"),
            "comp_fine_floor_value",
            self.witness.fine_floor,
            65,
        )?;
        let (fine_remainder, _) = allocate_ranged(
            cs.namespace(|| "comp_fine_remainder"),
            "comp_fine_remainder_value",
            self.witness.fine_remainder,
            bit_length_u128(divisor_u128 - 1),
        )?;
        cs.enforce(
            || "comp_scaled_product = divisor * floor + remainder",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (F::from(self.r_num), product.get_variable())
                    - (
                        field_from_u128::<F>(divisor_u128),
                        fine_floor.get_variable(),
                    )
                    - fine_remainder.get_variable()
            },
        );
        let fine_round_up = allocate_boolean_num(
            cs.namespace(|| "comp_fine_round_up"),
            "comp_fine_round_up_value",
            self.witness.fine_rounded_up,
        )?;
        enforce_constant_divisor_rounding(
            cs.namespace(|| "comp_fine_rounding_rule"),
            ConstantDivisorRoundingArgs {
                mode: self.rounding,
                floor_witness: self.witness.fine_floor,
                remainder_witness: self.witness.fine_remainder,
                divisor: divisor_u128,
                floor_bits: &fine_floor_bits,
                remainder: &fine_remainder,
                round_up: &fine_round_up,
            },
        )?;
        let fine_output = AllocatedNum::alloc(cs.namespace(|| "comp_fine_output"), || {
            Ok(field_from_u128(
                self.witness.fine_floor + u128::from(self.witness.fine_rounded_up),
            ))
        })?;
        cs.enforce(
            || "comp_fine_output = floor + round_up",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + fine_floor.get_variable() + fine_round_up.get_variable()
                    - fine_output.get_variable()
            },
        );

        let mut output_limbs = Vec::with_capacity(2);
        for (index, &limb) in self.witness.output_limbs.iter().enumerate() {
            let (limb, _) = allocate_ranged(
                cs.namespace(|| format!("comp_output_limb_{index}")),
                "comp_output_limb_value",
                limb as u128,
                16,
            )?;
            output_limbs.push(limb);
        }
        let output_is_one = allocate_boolean_num(
            cs.namespace(|| "comp_output_is_one"),
            "comp_output_is_one_value",
            self.witness.output_is_one,
        )?;
        for (index, limb) in output_limbs.iter().enumerate() {
            cs.enforce(
                || format!("comp_canonical_output_one_limb_{index}"),
                |lc| lc + output_is_one.get_variable(),
                |lc| lc + limb.get_variable(),
                |lc| lc,
            );
        }
        let output_residual_offset =
            AllocatedNum::alloc(cs.namespace(|| "comp_output_residual_offset"), || {
                Ok(F::from(self.witness.output_residual_offset))
            })?;
        range_check(
            cs.namespace(|| "comp_output_residual_offset_range"),
            &output_residual_offset,
            self.witness.output_residual_offset as u128,
            33,
        )?;
        let output_num = AllocatedNum::alloc(cs.namespace(|| "comp_output_numerator"), || {
            let value = output_limbs[0]
                .get_value()
                .zip(output_limbs[1].get_value())
                .zip(output_is_one.get_value())
                .map(|((hi, lo), is_one)| hi * F::from(1_u64 << 16) + lo + is_one * scale);
            value.ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "comp_reconstruct_output_numerator",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (F::from(1_u64 << 16), output_limbs[0].get_variable())
                    + output_limbs[1].get_variable()
                    + (scale, output_is_one.get_variable())
                    - output_num.get_variable()
            },
        );
        cs.enforce(
            || "comp_output_fine = S*V + offset - half",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (scale, output_num.get_variable()) + output_residual_offset.get_variable()
                    - (half, CS::one())
                    - fine_output.get_variable()
            },
        );

        // A decisão compensada compara na grade fina `2^64`, que é onde o
        // valor do estado realmente vive; comparar só em `V` descartaria o
        // residual e desperdiçaria a precisão que a representação compra.
        let decision = enforce_threshold_decision(
            cs.namespace(|| "comp_decision"),
            "comp_decision",
            &fine_output,
            self.witness.fine_floor + u128::from(self.witness.fine_rounded_up),
            self.threshold_fine_numerator,
            65,
        )?;

        let next_iteration = AllocatedNum::alloc(cs.namespace(|| "comp_next_iteration"), || {
            z[0].get_value()
                .map(|value| value + F::ONE)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "comp_increment_iteration",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );

        Ok(vec![
            next_iteration,
            output_limbs[0].clone(),
            output_limbs[1].clone(),
            output_is_one,
            output_residual_offset,
            decision,
        ])
    }
}

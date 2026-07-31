//! Referência FP32 para o subconjunto positivo usado nos workloads.

use serde::{Deserialize, Serialize};
use std::hint::black_box;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RestrictedFp32 {
    bits: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RestrictedFp32Trace {
    pub input: RestrictedFp32,
    pub complement: RestrictedFp32,
    pub product: RestrictedFp32,
    pub parameter: RestrictedFp32,
    pub output: RestrictedFp32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RestrictedFp32StableTrace {
    pub input: RestrictedFp32,
    pub alpha: RestrictedFp32,
    pub scaled: RestrictedFp32,
    pub innovation: RestrictedFp32,
    pub output: RestrictedFp32,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RestrictedFp32Error {
    #[error("sinal negativo não pertence ao perfil FP32 restrito")]
    Negative,
    #[error("NaN ou infinito não pertence ao perfil FP32 restrito")]
    NonFinite,
    #[error("subnormal não pertence ao perfil FP32 restrito")]
    Subnormal,
    #[error("valor fora do domínio público")]
    OutsideDomain,
}

impl RestrictedFp32 {
    pub fn from_bits(bits: u32) -> Result<Self, RestrictedFp32Error> {
        if bits >> 31 != 0 {
            return Err(RestrictedFp32Error::Negative);
        }
        let exponent = ((bits >> 23) & 0xff) as u8;
        let fraction = bits & 0x7f_ffff;
        if exponent == 0xff {
            return Err(RestrictedFp32Error::NonFinite);
        }
        if exponent == 0 && fraction != 0 {
            return Err(RestrictedFp32Error::Subnormal);
        }
        Ok(Self { bits })
    }

    pub fn from_f32(value: f32) -> Result<Self, RestrictedFp32Error> {
        Self::from_bits(value.to_bits())
    }

    pub fn bits(self) -> u32 {
        self.bits
    }

    pub fn value(self) -> f32 {
        f32::from_bits(self.bits)
    }

    pub fn exponent(self) -> u8 {
        ((self.bits >> 23) & 0xff) as u8
    }

    pub fn significand(self) -> u32 {
        if self.exponent() == 0 {
            0
        } else {
            (1 << 23) | (self.bits & 0x7f_ffff)
        }
    }

    pub fn is_zero(self) -> bool {
        self.bits == 0
    }
}

/// Executa exatamente a ordem do baseline do experimento, com uma barreira
/// entre operações para impedir FMA.
pub fn restricted_logistic_step(
    input_bits: u32,
    parameter_bits: u32,
) -> Result<RestrictedFp32Trace, RestrictedFp32Error> {
    let input = RestrictedFp32::from_bits(input_bits)?;
    let parameter = RestrictedFp32::from_bits(parameter_bits)?;
    if !(0.0..=1.0).contains(&input.value()) || !(0.0..=4.0).contains(&parameter.value()) {
        return Err(RestrictedFp32Error::OutsideDomain);
    }
    let complement_value = black_box(1.0_f32 - black_box(input.value()));
    let product_value = black_box(black_box(input.value()) * black_box(complement_value));
    let output_value = black_box(black_box(product_value) * black_box(parameter.value()));
    Ok(RestrictedFp32Trace {
        input,
        complement: RestrictedFp32::from_f32(complement_value)?,
        product: RestrictedFp32::from_f32(product_value)?,
        parameter,
        output: RestrictedFp32::from_f32(output_value)?,
    })
}

pub fn restricted_stable_step(
    input_bits: u32,
    alpha_bits: u32,
    innovation_bits: u32,
) -> Result<RestrictedFp32StableTrace, RestrictedFp32Error> {
    let input = RestrictedFp32::from_bits(input_bits)?;
    let alpha = RestrictedFp32::from_bits(alpha_bits)?;
    let innovation = RestrictedFp32::from_bits(innovation_bits)?;
    if !(0.0..=2.0).contains(&input.value())
        || !(0.0..=1.0).contains(&alpha.value())
        || !(0.0..=1.0).contains(&innovation.value())
    {
        return Err(RestrictedFp32Error::OutsideDomain);
    }
    let scaled_value = black_box(black_box(input.value()) * black_box(alpha.value()));
    let output_value = black_box(black_box(scaled_value) + black_box(innovation.value()));
    Ok(RestrictedFp32StableTrace {
        input,
        alpha,
        scaled: RestrictedFp32::from_f32(scaled_value)?,
        innovation,
        output: RestrictedFp32::from_f32(output_value)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use softfloat::F32;

    #[test]
    fn rejects_values_outside_restricted_profile() {
        assert_eq!(
            RestrictedFp32::from_bits((-1.0_f32).to_bits()),
            Err(RestrictedFp32Error::Negative)
        );
        assert_eq!(
            RestrictedFp32::from_bits(f32::INFINITY.to_bits()),
            Err(RestrictedFp32Error::NonFinite)
        );
        assert_eq!(
            RestrictedFp32::from_bits(1),
            Err(RestrictedFp32Error::Subnormal)
        );
    }

    #[test]
    fn decoded_parts_are_canonical() {
        let one = RestrictedFp32::from_f32(1.0).unwrap();
        assert_eq!(one.exponent(), 127);
        assert_eq!(one.significand(), 1 << 23);
        assert_eq!(RestrictedFp32::from_f32(0.0).unwrap().significand(), 0);
    }

    #[test]
    fn native_trace_matches_independent_software_float() {
        let one = F32::from_native_f32(1.0);
        let parameter_bits = 3.7_f32.to_bits();
        let parameter = F32::from_native_f32(f32::from_bits(parameter_bits));
        let mut state = 0x05ee_d754_u32;

        for _ in 0..10_000 {
            // Gera positivos normais em [2^-126,1), cobrindo expoentes e
            // mantissas sem produzir subnormais na entrada.
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let exponent = 1 + ((state >> 24) % 126);
            let fraction = state & 0x7f_ffff;
            let input_bits = (exponent << 23) | fraction;
            let native = restricted_logistic_step(input_bits, parameter_bits).unwrap();

            let input = F32::from_native_f32(f32::from_bits(input_bits));
            let complement = one.sub(input);
            let product = input.mul(complement);
            let output = product.mul(parameter);

            assert_eq!(
                native.complement.bits(),
                complement.to_native_f32().to_bits()
            );
            assert_eq!(native.product.bits(), product.to_native_f32().to_bits());
            assert_eq!(native.output.bits(), output.to_native_f32().to_bits());
        }
    }
}

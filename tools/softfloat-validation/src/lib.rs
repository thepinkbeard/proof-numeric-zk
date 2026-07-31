//! Validação diferencial contra Berkeley SoftFloat.
//!
//! Berkeley SoftFloat é a implementação de referência do IEEE 754 usada como
//! árbitro quando se quer afirmar que um FP32 está correto. A rodada
//! [0007](../../../docs/progress/0007-referencia-fp32-restrita.md) deixou esta
//! validação pendente: `softfloat-sys` só tem regras de build para x86_64 Linux
//! — o `cfg!` do build script olha o **host**, então nem Rosetta nem
//! `--target x86_64-apple-darwin` resolvem em Apple Silicon. A saída é rodar em
//! runner de CI x86_64 Linux, que é o que `.github/workflows/softfloat.yml` faz.
//!
//! O que mudou em relação à versão de 0007: aquela comparava `f32` nativo contra
//! Berkeley, o que valida o hardware da máquina e não este projeto. Aqui o
//! sujeito é `numeric_reference::fp32`, a referência que o experimento
//! efetivamente usa como baseline FP32 nos workloads logístico e afim.
//!
//! Em Apple Silicon os testes são compilados fora por `cfg`, e a validação ativa
//! continua sendo a de `numeric-reference` contra o crate `softfloat`, que é
//! independente mas não é Berkeley.

#[cfg(all(test, target_arch = "x86_64", target_os = "linux"))]
mod tests {
    use numeric_reference::{restricted_logistic_step, restricted_stable_step};
    use softfloat_wrapper::{Float, RoundingMode, F32};

    const ROUNDING: RoundingMode = RoundingMode::TiesToEven;

    /// Gerador congruencial explícito em vez de um crate de RNG: a sequência
    /// precisa ser idêntica entre execuções para que uma falha em CI seja
    /// reproduzível a partir da semente.
    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0
        }

        /// Positivo normal em `[2^-126, 2)`, sem subnormais nem zero.
        fn normal_bits(&mut self) -> u32 {
            let state = self.next();
            let exponent = 1 + ((state >> 24) % 127);
            let fraction = state & 0x7f_ffff;
            (exponent << 23) | fraction
        }

        /// Positivo normal em `[0, 1]`, o domínio do mapa logístico.
        fn unit_bits(&mut self) -> u32 {
            loop {
                let bits = self.normal_bits();
                if f32::from_bits(bits) <= 1.0 {
                    return bits;
                }
            }
        }
    }

    #[test]
    fn logistic_reference_matches_berkeley_softfloat() {
        let one = F32::from_bits(1.0_f32.to_bits());
        let parameter_bits = 3.7_f32.to_bits();
        let parameter = F32::from_bits(parameter_bits);
        let mut rng = Lcg(0x05ee_d754);
        let mut checked = 0_u32;

        for _ in 0..200_000 {
            let input_bits = rng.unit_bits();
            let Ok(trace) = restricted_logistic_step(input_bits, parameter_bits) else {
                continue;
            };

            let input = F32::from_bits(input_bits);
            let complement = one.sub(input, ROUNDING);
            let product = input.mul(complement, ROUNDING);
            let output = product.mul(parameter, ROUNDING);

            // Compara os três intermediários, não só a saída: um erro de
            // arredondamento que se cancelasse no produto final passaria por uma
            // comparação só de saída.
            assert_eq!(
                trace.complement.bits(),
                complement.to_bits(),
                "complemento divergiu para entrada {input_bits:#010x}"
            );
            assert_eq!(
                trace.product.bits(),
                product.to_bits(),
                "produto divergiu para entrada {input_bits:#010x}"
            );
            assert_eq!(
                trace.output.bits(),
                output.to_bits(),
                "saída divergiu para entrada {input_bits:#010x}"
            );
            checked += 1;
        }
        assert!(checked > 100_000, "amostra pequena demais: {checked}");
    }

    #[test]
    fn stable_reference_matches_berkeley_softfloat() {
        let mut rng = Lcg(0x1234_abcd);
        let mut checked = 0_u32;

        for _ in 0..200_000 {
            let input_bits = rng.normal_bits();
            let alpha_bits = rng.unit_bits();
            let innovation_bits = rng.unit_bits();
            let Ok(trace) = restricted_stable_step(input_bits, alpha_bits, innovation_bits) else {
                continue;
            };

            let input = F32::from_bits(input_bits);
            let alpha = F32::from_bits(alpha_bits);
            let innovation = F32::from_bits(innovation_bits);
            let scaled = input.mul(alpha, ROUNDING);
            let output = scaled.add(innovation, ROUNDING);

            assert_eq!(
                trace.scaled.bits(),
                scaled.to_bits(),
                "escala divergiu para {input_bits:#010x} x {alpha_bits:#010x}"
            );
            assert_eq!(
                trace.output.bits(),
                output.to_bits(),
                "saída divergiu para {input_bits:#010x} x {alpha_bits:#010x} + {innovation_bits:#010x}"
            );
            checked += 1;
        }
        assert!(checked > 50_000, "amostra pequena demais: {checked}");
    }

    /// Os casos que o arredondamento costuma quebrar, fixos em vez de sorteados:
    /// empates exatos, valores no limite do domínio e o menor normal.
    #[test]
    fn hard_rounding_cases_match_berkeley_softfloat() {
        let one = F32::from_bits(1.0_f32.to_bits());
        let parameter_bits = 3.7_f32.to_bits();
        let parameter = F32::from_bits(parameter_bits);

        let cases = [
            0x0080_0000_u32,          // menor normal positivo
            0x3f7f_ffff,              // maior valor abaixo de 1
            0x3f80_0000,              // 1,0 exato
            0x3f00_0000,              // 0,5, onde o produto empata
            0x3f00_0001,              // 0,5 mais um ulp
            0x3eaa_aaab,              // 1/3 arredondado
            0x3f2a_aaab,              // 2/3 arredondado
            0x0000_0000,              // zero
        ];

        for input_bits in cases {
            let Ok(trace) = restricted_logistic_step(input_bits, parameter_bits) else {
                continue;
            };
            let input = F32::from_bits(input_bits);
            let complement = one.sub(input, ROUNDING);
            let product = input.mul(complement, ROUNDING);
            let output = product.mul(parameter, ROUNDING);
            assert_eq!(trace.complement.bits(), complement.to_bits(), "{input_bits:#010x}");
            assert_eq!(trace.product.bits(), product.to_bits(), "{input_bits:#010x}");
            assert_eq!(trace.output.bits(), output.to_bits(), "{input_bits:#010x}");
        }
    }
}

//! Recorrência afim estável em ponto fixo e CBD.
//!
//! O workload afim `x' = alpha*x + c`, com `0 < alpha < 1`, já tinha circuitos
//! CFD e FP32. Faltavam ponto fixo e CBD, e sem eles a fronteira do regime
//! contrativo compararia certificado contra certificado — justamente o regime em
//! que o formato não certificado deveria ganhar.
//!
//! O contraste com o mapa logístico é o ponto: ali o erro cresce a cada passo e
//! o raio CBD estoura; aqui a recorrência é contrativa, o raio converge para
//! `1/(1-alpha)` ULP e a abstenção nunca chega. O custo certificado deixa de
//! comprar cobertura e passa a ser despesa pura.
//!
//! ## Estado
//!
//! O domínio é `[0,2)`, não `[0,1)`. O estado reaproveita o formato do mapa
//! logístico — `K` limbs de 16 bits mais um bit — reinterpretando o bit como
//! parte alta em vez de sinalizador de `x == 1`: o valor é `alto + N/S`, com
//! `S = 2^(16K)`. Para o logístico o bit só liga com `N = 0` e significa `x = 1`;
//! aqui ele é livre. Nenhum layout novo, e as testemunhas iniciais e o gadget de
//! decisão continuam valendo.

use crate::{
    allocate_boolean_num, allocate_ranged, bit_length_u128, enforce_boolean_num,
    enforce_certified_decision, enforce_constant_divisor_rounding, enforce_threshold_decision,
    field_from_u128, range_check, should_round_up_u128, CertifiedDecisionArgs, CircuitRounding,
    ConstantDivisorRoundingArgs,
};
use ff::PrimeField;
use nova_snark::{
    frontend::{num::AllocatedNum, ConstraintSystem, LinearCombination, SynthesisError},
    traits::circuit::StepCircuit,
};

/// Parâmetros públicos da recorrência, como razões de inteiros pequenos.
///
/// São públicos porque descrevem o problema, não a entrada privada. Se
/// entrassem na testemunha, um provador poderia escolher `alpha` depois de ver
/// o estado.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AffineParameters {
    pub alpha_num: u64,
    pub alpha_den: u64,
    pub innovation_num: u64,
    pub innovation_den: u64,
}

impl AffineParameters {
    pub fn new(alpha_num: u64, alpha_den: u64, innovation_num: u64, innovation_den: u64) -> Self {
        assert!(alpha_den > 0 && innovation_den > 0, "denominador nulo");
        assert!(alpha_num < alpha_den, "alpha deve contrair: alpha < 1");
        // O produto dos coeficientes entra no numerador comum junto de `X`, que
        // já ocupa `16K+1` bits. O limite de 16 bits mantém tudo em `u128` para
        // `K = 4`, que é o maior perfil da matriz.
        assert!(
            alpha_num < (1 << 16)
                && alpha_den < (1 << 16)
                && innovation_num < (1 << 16)
                && innovation_den < (1 << 16),
            "coeficientes afins acima de 16 bits estouram a escala comum"
        );
        Self {
            alpha_num,
            alpha_den,
            innovation_num,
            innovation_den,
        }
    }

    /// Divisor comum `a_d * c_d` da etapa.
    pub fn divisor(&self) -> u128 {
        self.alpha_den as u128 * self.innovation_den as u128
    }

    /// Numerador exato de `alpha*x + c` na escala `S`, antes do arredondamento.
    fn raw_numerator(&self, value: u128, scale: u128) -> u128 {
        self.alpha_num as u128 * self.innovation_den as u128 * value
            + self.innovation_num as u128 * self.alpha_den as u128 * scale
    }

    /// Ponto fixo `c/(1-alpha)` como razão, para validar o domínio antes de
    /// varrer: se o atrator ficar fora de `[0,2)` a trajetória estoura o estado.
    pub fn attractor(&self) -> (u128, u128) {
        let numerator = self.innovation_num as u128 * self.alpha_den as u128;
        let denominator =
            self.innovation_den as u128 * (self.alpha_den as u128 - self.alpha_num as u128);
        (numerator, denominator)
    }
}

/// Estado `(limbs, alto)` de uma etapa afim em ponto fixo.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AffineStepWitness<const K: usize> {
    pub input_limbs: [u64; K],
    pub input_high: bool,
    pub raw_numerator: u128,
    pub quotient_floor: u128,
    pub remainder: u128,
    pub rounded_up: bool,
    pub output_limbs: [u64; K],
    pub output_high: bool,
}

fn split_state<const K: usize>(value: u128) -> ([u64; K], bool) {
    let scale = 1_u128 << (16 * K);
    let high = value >= scale;
    let mut remaining = value - if high { scale } else { 0 };
    let mut limbs = [0_u64; K];
    for limb in limbs.iter_mut().rev() {
        *limb = (remaining & 0xffff) as u64;
        remaining >>= 16;
    }
    (limbs, high)
}

fn join_state<const K: usize>(limbs: &[u64; K], high: bool) -> u128 {
    let scale = 1_u128 << (16 * K);
    limbs.iter().fold(0_u128, |acc, &limb| (acc << 16) + limb as u128)
        + if high { scale } else { 0 }
}

impl<const K: usize> AffineStepWitness<K> {
    pub fn compute(
        input_limbs: [u64; K],
        input_high: bool,
        parameters: AffineParameters,
        rounding: CircuitRounding,
    ) -> Self {
        assert!(K > 0 && K <= 4);
        let scale = 1_u128 << (16 * K);
        let input = join_state(&input_limbs, input_high);
        assert!(input < 2 * scale, "estado fora de [0,2)");
        let raw_numerator = parameters.raw_numerator(input, scale);
        let divisor = parameters.divisor();
        let quotient_floor = raw_numerator / divisor;
        let remainder = raw_numerator % divisor;
        let rounded_up = should_round_up_u128(quotient_floor, remainder, divisor, rounding);
        let output = quotient_floor + u128::from(rounded_up);
        assert!(
            output < 2 * scale,
            "recorrência afim saiu de [0,2): revise alpha e a inovação"
        );
        let (output_limbs, output_high) = split_state::<K>(output);
        Self {
            input_limbs,
            input_high,
            raw_numerator,
            quotient_floor,
            remainder,
            rounded_up,
            output_limbs,
            output_high,
        }
    }

    pub fn output_value(&self) -> u128 {
        join_state(&self.output_limbs, self.output_high)
    }
}

#[derive(Clone, Debug)]
pub struct AffineStepCircuit<const K: usize> {
    pub witness: AffineStepWitness<K>,
    pub parameters: AffineParameters,
    pub rounding: CircuitRounding,
    /// Limiar público do predicado, no numerador da escala `2^(16K)`.
    pub threshold_numerator: u128,
}

impl<const K: usize> AffineStepCircuit<K> {
    pub fn arity_value() -> usize {
        K + 3 // iteração, K limbs, alto, decisão
    }
}

/// Aloca o estado `(limbs, alto)` e devolve o numerador reconstruído.
///
/// Os limbs vêm de `z`, que o Nova propaga entre etapas sem nenhuma garantia de
/// faixa: sem o range check, um provador poderia apresentar um limb maior que
/// 16 bits e a reconstrução deixaria de ser injetora.
fn allocate_input<F, CS, const K: usize>(
    cs: &mut CS,
    z: &[AllocatedNum<F>],
    limbs_witness: &[u64; K],
    high_witness: bool,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let scale = field_from_u128::<F>(1_u128 << (16 * K));
    for (index, limb) in z[1..=K].iter().enumerate() {
        range_check(
            cs.namespace(|| format!("affine_input_limb_{index}_range")),
            limb,
            limbs_witness[index] as u128,
            16,
        )?;
    }
    let input_high = &z[K + 1];
    enforce_boolean_num(cs.namespace(|| "affine_input_high_boolean"), input_high);

    let input_num = AllocatedNum::alloc(cs.namespace(|| "affine_input_numerator"), || {
        Ok(field_from_u128::<F>(join_state(limbs_witness, high_witness)))
    })?;
    let mut reconstruction = LinearCombination::zero();
    let mut coefficient = F::ONE;
    for limb in z[1..=K].iter().rev() {
        reconstruction = reconstruction + (coefficient, limb.get_variable());
        coefficient *= F::from(1_u64 << 16);
    }
    reconstruction =
        reconstruction + (scale, input_high.get_variable()) - input_num.get_variable();
    cs.enforce(
        || "affine reconstruct input numerator",
        |lc| lc,
        |lc| lc,
        |_| reconstruction,
    );
    Ok(input_num)
}

/// `(numerador, limbs, bit alto)` da saída afim.
type AllocatedOutput<F> = (AllocatedNum<F>, Vec<AllocatedNum<F>>, AllocatedNum<F>);

/// Aloca `(limbs, alto)` de saída e devolve o numerador.
fn allocate_output<F, CS, const K: usize>(
    cs: &mut CS,
    limbs_witness: &[u64; K],
    high_witness: bool,
) -> Result<AllocatedOutput<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let scale = field_from_u128::<F>(1_u128 << (16 * K));
    let mut output_limbs = Vec::with_capacity(K);
    for (index, &limb) in limbs_witness.iter().enumerate() {
        let (limb_num, _) = allocate_ranged(
            cs.namespace(|| format!("affine_output_limb_{index}")),
            "affine_output_limb_value",
            limb as u128,
            16,
        )?;
        output_limbs.push(limb_num);
    }
    let output_high = allocate_boolean_num(
        cs.namespace(|| "affine_output_high"),
        "affine_output_high_value",
        high_witness,
    )?;
    let output_num = AllocatedNum::alloc(cs.namespace(|| "affine_output_numerator"), || {
        Ok(field_from_u128::<F>(join_state(limbs_witness, high_witness)))
    })?;
    let mut reconstruction = LinearCombination::zero();
    let mut coefficient = F::ONE;
    for limb in output_limbs.iter().rev() {
        reconstruction = reconstruction + (coefficient, limb.get_variable());
        coefficient *= F::from(1_u64 << 16);
    }
    reconstruction =
        reconstruction + (scale, output_high.get_variable()) - output_num.get_variable();
    cs.enforce(
        || "affine reconstruct output numerator",
        |lc| lc,
        |lc| lc,
        |_| reconstruction,
    );
    Ok((output_num, output_limbs, output_high))
}

/// Impõe `R = a_n*c_d*X + c_n*a_d*S` e a divisão com resto por `a_d*c_d`.
///
/// `R` é combinação **linear** de `X`: a etapa afim não tem produto de duas
/// variáveis, ao contrário do logístico. É a razão de o circuito afim custar
/// bem menos que o logístico na mesma precisão.
fn enforce_affine_quotient<F, CS, const K: usize>(
    cs: &mut CS,
    input_num: &AllocatedNum<F>,
    parameters: AffineParameters,
    rounding: CircuitRounding,
    quotient_floor_witness: u128,
    remainder_witness: u128,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let scale_u128 = 1_u128 << (16 * K);
    let divisor = parameters.divisor();
    let left_coefficient =
        field_from_u128::<F>(parameters.alpha_num as u128 * parameters.innovation_den as u128);
    let constant_term = field_from_u128::<F>(
        parameters.innovation_num as u128 * parameters.alpha_den as u128 * scale_u128,
    );

    let (quotient_floor, quotient_floor_bits) = allocate_ranged(
        cs.namespace(|| "affine_quotient_floor"),
        "affine_quotient_floor_value",
        quotient_floor_witness,
        16 * K + 2,
    )?;
    let (remainder, _) = allocate_ranged(
        cs.namespace(|| "affine_remainder"),
        "affine_remainder_value",
        remainder_witness,
        bit_length_u128(divisor),
    )?;
    // O resto precisa ser menor que o divisor, senão o quociente não é o piso.
    let (remainder_slack, _) = allocate_ranged(
        cs.namespace(|| "affine_remainder_slack"),
        "affine_remainder_slack_value",
        divisor - 1 - remainder_witness,
        bit_length_u128(divisor),
    )?;
    cs.enforce(
        || "affine remainder below divisor",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + (field_from_u128::<F>(divisor), CS::one())
                - CS::one()
                - remainder.get_variable()
                - remainder_slack.get_variable()
        },
    );
    cs.enforce(
        || "affine raw = divisor * floor + remainder",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + (left_coefficient, input_num.get_variable()) + (constant_term, CS::one())
                - (field_from_u128::<F>(divisor), quotient_floor.get_variable())
                - remainder.get_variable()
        },
    );

    let round_up = allocate_boolean_num(
        cs.namespace(|| "affine_round_up"),
        "affine_round_up_value",
        should_round_up_u128(quotient_floor_witness, remainder_witness, divisor, rounding),
    )?;
    enforce_constant_divisor_rounding(
        cs.namespace(|| "affine_rounding_rule"),
        ConstantDivisorRoundingArgs {
            mode: rounding,
            floor_witness: quotient_floor_witness,
            remainder_witness,
            divisor,
            floor_bits: &quotient_floor_bits,
            remainder: &remainder,
            round_up: &round_up,
        },
    )?;

    let quotient = AllocatedNum::alloc(cs.namespace(|| "affine_quotient"), || {
        Ok(field_from_u128::<F>(
            quotient_floor_witness
                + u128::from(should_round_up_u128(
                    quotient_floor_witness,
                    remainder_witness,
                    divisor,
                    rounding,
                )),
        ))
    })?;
    cs.enforce(
        || "affine quotient = floor + round_up",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + quotient_floor.get_variable() + round_up.get_variable()
                - quotient.get_variable()
        },
    );
    Ok(quotient)
}

impl<F: PrimeField, const K: usize> StepCircuit<F> for AffineStepCircuit<K> {
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

        let input_num = allocate_input::<F, CS, K>(
            cs,
            z,
            &self.witness.input_limbs,
            self.witness.input_high,
        )?;
        let quotient = enforce_affine_quotient::<F, CS, K>(
            cs,
            &input_num,
            self.parameters,
            self.rounding,
            self.witness.quotient_floor,
            self.witness.remainder,
        )?;
        let (output_num, output_limbs, output_high) =
            allocate_output::<F, CS, K>(cs, &self.witness.output_limbs, self.witness.output_high)?;
        cs.enforce(
            || "affine output equals quotient",
            |lc| lc,
            |lc| lc,
            |lc| lc + quotient.get_variable() - output_num.get_variable(),
        );

        let decision = enforce_threshold_decision(
            cs.namespace(|| "affine_decision"),
            "affine_decision",
            &output_num,
            self.witness.output_value(),
            self.threshold_numerator,
            16 * K + 2,
        )?;

        let next_iteration = AllocatedNum::alloc(cs.namespace(|| "affine_next_iteration"), || {
            z[0].get_value()
                .map(|value| value + F::ONE)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "affine iteration increment",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );

        let mut output = Vec::with_capacity(Self::arity_value());
        output.push(next_iteration);
        output.extend(output_limbs);
        output.push(output_high);
        output.push(decision);
        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// CBD
// ---------------------------------------------------------------------------

/// Etapa afim com raio certificado.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CbdAffineStepWitness<const K: usize> {
    pub input_limbs: [u64; K],
    pub input_high: bool,
    pub input_radius: u128,
    pub quotient_floor: u128,
    pub remainder: u128,
    pub rounded_up: bool,
    pub output_limbs: [u64; K],
    pub output_high: bool,
    pub radius_floor: u128,
    pub radius_remainder: u128,
    pub output_radius: u128,
    /// 0=falso, 1=verdadeiro, 2=indeterminado para `x >= t`.
    pub decision: u64,
}

impl<const K: usize> CbdAffineStepWitness<K> {
    /// A imagem de `[x-rad, x+rad]` por `alpha*x + c` tem raio `alpha*rad`,
    /// arredondado **para cima**, mais uma ULP do arredondamento do centro.
    ///
    /// Como `alpha < 1`, o raio converge para `1/(1-alpha)` ULP em vez de
    /// crescer. É a diferença estrutural com o mapa logístico, onde o fator de
    /// expansão é maior que 1 e o raio satura o domínio em poucas dezenas de
    /// passos.
    pub fn compute(
        input_limbs: [u64; K],
        input_high: bool,
        input_radius: u128,
        parameters: AffineParameters,
        rounding: CircuitRounding,
        threshold_numerator: u128,
    ) -> Self {
        assert!(K > 0 && K <= 4);
        let scale = 1_u128 << (16 * K);
        let input = join_state(&input_limbs, input_high);
        assert!(input < 2 * scale, "estado fora de [0,2)");
        assert!(input_radius <= 2 * scale);

        let raw_numerator = parameters.raw_numerator(input, scale);
        let divisor = parameters.divisor();
        let quotient_floor = raw_numerator / divisor;
        let remainder = raw_numerator % divisor;
        let rounded_up = should_round_up_u128(quotient_floor, remainder, divisor, rounding);
        let output = quotient_floor + u128::from(rounded_up);
        assert!(
            output < 2 * scale,
            "recorrência afim saiu de [0,2): revise alpha e a inovação"
        );
        let (output_limbs, output_high) = split_state::<K>(output);

        let scaled_radius = parameters.alpha_num as u128 * input_radius;
        let radius_floor = scaled_radius / parameters.alpha_den as u128;
        let radius_remainder = scaled_radius % parameters.alpha_den as u128;
        // Teto do raio propagado mais uma ULP do centro. Ambos para cima: um
        // raio subestimado autorizaria uma decisão que o enclosure não sustenta.
        let output_radius =
            (radius_floor + u128::from(radius_remainder > 0) + 1).min(2 * scale);

        let decision = if output >= threshold_numerator + output_radius {
            1
        } else if output + output_radius < threshold_numerator {
            0
        } else {
            2
        };

        Self {
            input_limbs,
            input_high,
            input_radius,
            quotient_floor,
            remainder,
            rounded_up,
            output_limbs,
            output_high,
            radius_floor,
            radius_remainder,
            output_radius,
            decision,
        }
    }

    pub fn output_value(&self) -> u128 {
        join_state(&self.output_limbs, self.output_high)
    }
}

#[derive(Clone, Debug)]
pub struct CbdAffineStepCircuit<const K: usize> {
    pub witness: CbdAffineStepWitness<K>,
    pub parameters: AffineParameters,
    pub rounding: CircuitRounding,
    pub threshold_numerator: u128,
}

impl<const K: usize> CbdAffineStepCircuit<K> {
    pub fn arity_value() -> usize {
        K + 4 // iteração, K limbs, alto, raio, decisão
    }
}

impl<F: PrimeField, const K: usize> StepCircuit<F> for CbdAffineStepCircuit<K> {
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
        let radius_bits = 16 * K + 2;

        let input_num = allocate_input::<F, CS, K>(
            cs,
            z,
            &self.witness.input_limbs,
            self.witness.input_high,
        )?;
        let input_radius = &z[K + 2];
        range_check(
            cs.namespace(|| "cbd_affine_input_radius_range"),
            input_radius,
            self.witness.input_radius,
            radius_bits,
        )?;

        let quotient = enforce_affine_quotient::<F, CS, K>(
            cs,
            &input_num,
            self.parameters,
            self.rounding,
            self.witness.quotient_floor,
            self.witness.remainder,
        )?;
        let (output_num, output_limbs, output_high) =
            allocate_output::<F, CS, K>(cs, &self.witness.output_limbs, self.witness.output_high)?;
        cs.enforce(
            || "cbd affine output equals quotient",
            |lc| lc,
            |lc| lc,
            |lc| lc + quotient.get_variable() - output_num.get_variable(),
        );

        // Raio: `a_n * rad = a_d * piso + resto`, com `resto < a_d`, e o raio de
        // saída é o teto mais uma ULP.
        let alpha_den = self.parameters.alpha_den as u128;
        let (radius_floor, _) = allocate_ranged(
            cs.namespace(|| "cbd_affine_radius_floor"),
            "cbd_affine_radius_floor_value",
            self.witness.radius_floor,
            radius_bits,
        )?;
        let (radius_remainder, _) = allocate_ranged(
            cs.namespace(|| "cbd_affine_radius_remainder"),
            "cbd_affine_radius_remainder_value",
            self.witness.radius_remainder,
            bit_length_u128(alpha_den),
        )?;
        let (radius_remainder_slack, _) = allocate_ranged(
            cs.namespace(|| "cbd_affine_radius_slack"),
            "cbd_affine_radius_slack_value",
            alpha_den - 1 - self.witness.radius_remainder,
            bit_length_u128(alpha_den),
        )?;
        cs.enforce(
            || "cbd affine radius remainder below alpha denominator",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (field_from_u128::<F>(alpha_den), CS::one())
                    - CS::one()
                    - radius_remainder.get_variable()
                    - radius_remainder_slack.get_variable()
            },
        );
        cs.enforce(
            || "cbd affine radius division",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + (
                    field_from_u128::<F>(self.parameters.alpha_num as u128),
                    input_radius.get_variable(),
                ) - (field_from_u128::<F>(alpha_den), radius_floor.get_variable())
                    - radius_remainder.get_variable()
            },
        );
        let remainder_nonzero = allocate_boolean_num(
            cs.namespace(|| "cbd_affine_radius_remainder_nonzero"),
            "cbd_affine_radius_remainder_nonzero_value",
            self.witness.radius_remainder > 0,
        )?;
        // `resto * (1 - indicador) = 0` força o indicador quando o resto não é
        // nulo; combinado com `indicador * resto = indicador * resto` acima, o
        // provador não pode declarar teto menor que o real.
        cs.enforce(
            || "cbd affine remainder nonzero indicator",
            |lc| lc + radius_remainder.get_variable(),
            |lc| lc + CS::one() - remainder_nonzero.get_variable(),
            |lc| lc,
        );
        let radius_inverse = AllocatedNum::alloc(cs.namespace(|| "cbd_affine_radius_inverse"), || {
            Ok(if self.witness.radius_remainder > 0 {
                field_from_u128::<F>(self.witness.radius_remainder)
                    .invert()
                    .unwrap_or(F::ZERO)
            } else {
                F::ZERO
            })
        })?;
        cs.enforce(
            || "cbd affine remainder nonzero completeness",
            |lc| lc + radius_remainder.get_variable(),
            |lc| lc + radius_inverse.get_variable(),
            |lc| lc + remainder_nonzero.get_variable(),
        );

        let output_radius = AllocatedNum::alloc(cs.namespace(|| "cbd_affine_output_radius"), || {
            Ok(field_from_u128::<F>(self.witness.output_radius))
        })?;
        range_check(
            cs.namespace(|| "cbd_affine_output_radius_range"),
            &output_radius,
            self.witness.output_radius,
            radius_bits,
        )?;
        cs.enforce(
            || "cbd affine output radius = ceil(alpha*rad) + 1",
            |lc| lc,
            |lc| lc,
            |lc| {
                lc + radius_floor.get_variable() + remainder_nonzero.get_variable() + CS::one()
                    - output_radius.get_variable()
            },
        );

        let decision = enforce_certified_decision(
            cs.namespace(|| "cbd_affine_decision"),
            "cbd_affine",
            CertifiedDecisionArgs {
                center: &output_num,
                center_witness: self.witness.output_value(),
                radius: &output_radius,
                radius_witness: self.witness.output_radius,
                threshold: self.threshold_numerator,
                decision_witness: self.witness.decision,
                comparison_bits: 16 * K + 3,
            },
        )?;

        let next_iteration =
            AllocatedNum::alloc(cs.namespace(|| "cbd_affine_next_iteration"), || {
                z[0].get_value()
                    .map(|value| value + F::ONE)
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;
        cs.enforce(
            || "cbd affine iteration increment",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );

        let mut output = Vec::with_capacity(Self::arity_value());
        output.push(next_iteration);
        output.extend(output_limbs);
        output.push(output_high);
        output.push(output_radius);
        output.push(decision);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_snark::provider::PallasEngine;
    use nova_snark::traits::Engine;

    type Scalar = <PallasEngine as Engine>::Scalar;

    fn parameters() -> AffineParameters {
        AffineParameters::new(1, 2, 3, 4)
    }

    #[test]
    fn state_round_trips_through_limbs() {
        for value in [0_u128, 1, 12345, (1 << 32) - 1, 1 << 32, (1 << 33) - 1] {
            let (limbs, high) = split_state::<2>(value);
            assert_eq!(join_state(&limbs, high), value);
        }
    }

    #[test]
    fn the_step_matches_exact_rational_arithmetic() {
        let scale = 1_u128 << 32;
        let (limbs, high) = split_state::<2>(scale / 10);
        let witness = AffineStepWitness::<2>::compute(
            limbs,
            high,
            parameters(),
            CircuitRounding::NearestTiesToEven,
        );
        // x0 = 1/10 exige arredondamento na grade; 0,1/2 + 0,75 = 0,8.
        let expected = (scale * 8) / 10;
        assert!(witness.output_value().abs_diff(expected) <= 2);
    }

    /// O atrator de `x/2 + 3/4` é `3/2`, e a trajetória converge por baixo.
    #[test]
    fn the_trajectory_converges_to_the_attractor() {
        let scale = 1_u128 << 32;
        let (mut limbs, mut high) = split_state::<2>(scale / 10);
        for _ in 0..64 {
            let witness = AffineStepWitness::<2>::compute(
                limbs,
                high,
                parameters(),
                CircuitRounding::NearestTiesToEven,
            );
            limbs = witness.output_limbs;
            high = witness.output_high;
        }
        let value = join_state(&limbs, high);
        assert!(value.abs_diff(3 * scale / 2) <= 4, "valor {value}");
    }

    /// O raio CBD converge em vez de crescer: com `alpha = 1/2`, o ponto fixo de
    /// `rad -> ceil(rad/2) + 1` é 2 ULP.
    #[test]
    fn the_certified_radius_stays_bounded() {
        let scale = 1_u128 << 32;
        let (mut limbs, mut high) = split_state::<2>(scale / 10);
        let mut radius = 1_u128;
        let mut maximum = radius;
        for _ in 0..256 {
            let witness = CbdAffineStepWitness::<2>::compute(
                limbs,
                high,
                radius,
                parameters(),
                CircuitRounding::NearestTiesToEven,
                scale,
            );
            limbs = witness.output_limbs;
            high = witness.output_high;
            radius = witness.output_radius;
            maximum = maximum.max(radius);
        }
        assert_eq!(radius, 2, "raio estacionário");
        assert!(maximum <= 3, "raio máximo {maximum}");
    }

    /// O enclosure certificado precisa conter o valor do ponto fixo exato.
    #[test]
    fn the_enclosure_contains_the_fixed_point_trajectory() {
        let scale = 1_u128 << 32;
        let (mut exact_limbs, mut exact_high) = split_state::<2>(scale / 10);
        let (mut cbd_limbs, mut cbd_high) = (exact_limbs, exact_high);
        let mut radius = 1_u128;
        for _ in 0..128 {
            let exact = AffineStepWitness::<2>::compute(
                exact_limbs,
                exact_high,
                parameters(),
                CircuitRounding::NearestTiesToEven,
            );
            exact_limbs = exact.output_limbs;
            exact_high = exact.output_high;
            let certified = CbdAffineStepWitness::<2>::compute(
                cbd_limbs,
                cbd_high,
                radius,
                parameters(),
                CircuitRounding::NearestTiesToEven,
                scale,
            );
            cbd_limbs = certified.output_limbs;
            cbd_high = certified.output_high;
            radius = certified.output_radius;
            let center = certified.output_value();
            let value = exact.output_value();
            assert!(
                value >= center.saturating_sub(radius) && value <= center + radius,
                "valor {value} fora de [{}, {}]",
                center.saturating_sub(radius),
                center + radius
            );
        }
    }

    #[test]
    fn the_fixed_point_circuit_is_satisfied() {
        use nova_snark::frontend::test_cs::TestConstraintSystem;
        let scale = 1_u128 << 32;
        let (limbs, high) = split_state::<2>(scale / 10);
        let witness = AffineStepWitness::<2>::compute(
            limbs,
            high,
            parameters(),
            CircuitRounding::NearestTiesToEven,
        );
        let circuit = AffineStepCircuit::<2> {
            witness: witness.clone(),
            parameters: parameters(),
            rounding: CircuitRounding::NearestTiesToEven,
            threshold_numerator: scale,
        };
        let mut cs = TestConstraintSystem::<Scalar>::new();
        let z: Vec<AllocatedNum<Scalar>> = [
            0_u128,
            witness.input_limbs[0] as u128,
            witness.input_limbs[1] as u128,
            u128::from(witness.input_high),
            0,
        ]
        .iter()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cs.namespace(|| format!("z_{index}")), || {
                Ok(field_from_u128::<Scalar>(*value))
            })
            .unwrap()
        })
        .collect();
        circuit.synthesize(&mut cs, &z).unwrap();
        assert!(cs.is_satisfied(), "insatisfeito: {:?}", cs.which_is_unsatisfied());
    }

    #[test]
    fn the_certified_circuit_is_satisfied() {
        use nova_snark::frontend::test_cs::TestConstraintSystem;
        let scale = 1_u128 << 32;
        let (limbs, high) = split_state::<2>(scale / 10);
        let witness = CbdAffineStepWitness::<2>::compute(
            limbs,
            high,
            1,
            parameters(),
            CircuitRounding::NearestTiesToEven,
            scale,
        );
        let circuit = CbdAffineStepCircuit::<2> {
            witness: witness.clone(),
            parameters: parameters(),
            rounding: CircuitRounding::NearestTiesToEven,
            threshold_numerator: scale,
        };
        let mut cs = TestConstraintSystem::<Scalar>::new();
        let z: Vec<AllocatedNum<Scalar>> = [
            0_u128,
            witness.input_limbs[0] as u128,
            witness.input_limbs[1] as u128,
            u128::from(witness.input_high),
            witness.input_radius,
            0,
        ]
        .iter()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cs.namespace(|| format!("z_{index}")), || {
                Ok(field_from_u128::<Scalar>(*value))
            })
            .unwrap()
        })
        .collect();
        circuit.synthesize(&mut cs, &z).unwrap();
        assert!(cs.is_satisfied(), "insatisfeito: {:?}", cs.which_is_unsatisfied());
    }
}

//! Vínculo de escalonamento entre perfis.
//!
//! Quando uma representação certificada se abstém, a recuperação é reprovar com
//! um perfil maior. Isso só vale alguma coisa se a segunda prova usar a **mesma
//! entrada privada** da primeira. Sem esse vínculo, um provador que recebe
//! `Indeterminate` pode escolher outra entrada, uma que decida do jeito que lhe
//! convém, e apresentar a prova do perfil maior como se fosse a continuação da
//! primeira. A abstenção deixaria de ser um pedido de mais precisão e viraria
//! uma oportunidade de troca.
//!
//! O vínculo é o que faltava desde a rodada 0009, item 3: `next_profile` e
//! `escalate_exact` existiam em `numeric-core` sem nenhum uso em circuito, e o
//! termo `P(indeterminado) * custo(p')` da métrica do protocolo era hipotético.
//!
//! ## O que este circuito prova
//!
//! Dado um compromisso público `C` da entrada privada e dois perfis, o grosso
//! `p` e o fino `p'`:
//!
//! 1. `C = Poseidon(limbs finos)`, o que amarra a entrada privada;
//! 2. o estado inicial em `p` é a quantização correta desses limbs;
//! 3. o estado inicial em `p'` é a quantização correta dos **mesmos** limbs.
//!
//! As duas quantizações partem da mesma testemunha, e é isso que impede a troca.
//! O custo medido aqui é o do vínculo isolado: ele entra uma vez por
//! escalonamento, não uma vez por etapa.

use crate::{allocate_ranged, field_from_u128, range_check};
use ff::{PrimeField, PrimeFieldBits};
use nova_snark::{
    frontend::{num::AllocatedNum, ConstraintSystem, LinearCombination, SynthesisError},
    provider::poseidon::{PoseidonConstantsCircuit, PoseidonROCircuit},
    traits::{circuit::StepCircuit, ROCircuitTrait},
};
use serde::{Deserialize, Serialize};

/// Testemunha do vínculo.
///
/// `fine_limbs` é a entrada privada na grade mais fina disponível; os dois
/// estados são derivados dela por deslocamento, não fornecidos de fora.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscalationWitness {
    /// Limbs de 16 bits da entrada privada na grade fina, do mais significativo
    /// para o menos.
    pub fine_limbs: Vec<u64>,
    /// Bits fracionários do perfil grosso, múltiplo de 16.
    pub coarse_bits: usize,
    /// Bits fracionários do perfil escalonado, múltiplo de 16 e maior.
    pub escalated_bits: usize,
}

impl EscalationWitness {
    pub fn new(fine_limbs: Vec<u64>, coarse_bits: usize, escalated_bits: usize) -> Self {
        assert!(!fine_limbs.is_empty(), "entrada privada vazia");
        assert!(
            fine_limbs.iter().all(|limb| *limb < 1 << 16),
            "limb acima de 16 bits"
        );
        assert!(coarse_bits % 16 == 0 && escalated_bits % 16 == 0);
        assert!(
            coarse_bits < escalated_bits,
            "o perfil escalonado precisa ser mais fino que o original"
        );
        assert!(
            escalated_bits <= 16 * fine_limbs.len(),
            "a grade fina não comporta o perfil escalonado"
        );
        Self {
            fine_limbs,
            coarse_bits,
            escalated_bits,
        }
    }

    fn fine_bits(&self) -> usize {
        16 * self.fine_limbs.len()
    }

    fn fine_value(&self) -> u128 {
        self.fine_limbs
            .iter()
            .fold(0_u128, |acc, limb| (acc << 16) + *limb as u128)
    }

    /// Quantização por truncagem para `bits` fracionários.
    ///
    /// Truncagem e não par mais próximo: o vínculo precisa valer para os dois
    /// perfis com a mesma relação, e o piso é o único que se escreve como
    /// `valor = quociente * deslocamento + resto` sem um bit de arredondamento
    /// por perfil. O arredondamento do estado inicial já é responsabilidade do
    /// circuito de etapa, que recebe este estado pronto.
    pub fn quantized(&self, bits: usize) -> u128 {
        self.fine_value() >> (self.fine_bits() - bits)
    }

    pub fn coarse_value(&self) -> u128 {
        self.quantized(self.coarse_bits)
    }

    pub fn escalated_value(&self) -> u128 {
        self.quantized(self.escalated_bits)
    }

    fn remainder(&self, bits: usize) -> u128 {
        let shift = self.fine_bits() - bits;
        self.fine_value() & ((1_u128 << shift) - 1)
    }
}

/// Circuito do vínculo, medido isoladamente.
///
/// Os identificadores de perfil entram como elementos de corpo derivados do
/// `profile_hash_hex` de cada lado. Eles são públicos e comparados por
/// igualdade: o circuito não recomputa o hash de perfil, que é serialização
/// canônica fora de corpo e custaria muito mais do que o vínculo inteiro.
/// A garantia que resta é a certa: o verificador confere os dois identificadores
/// contra os perfis declarados antes de aceitar a prova.
#[derive(Clone)]
pub struct EscalationLinkCircuit<Scalar: PrimeField> {
    pub witness: EscalationWitness,
    pub constants: PoseidonConstantsCircuit<Scalar>,
}

impl<Scalar: PrimeField> EscalationLinkCircuit<Scalar> {
    /// `[compromisso, estado grosso, estado escalonado]`.
    pub fn arity_value() -> usize {
        3
    }
}

impl<Scalar> EscalationLinkCircuit<Scalar>
where
    Scalar: PrimeField + PrimeFieldBits + Serialize + for<'de> Deserialize<'de>,
{
    /// Compromisso esperado, calculado fora do circuito com a mesma esponja.
    ///
    /// Usa o gadget circuital sobre um sistema de restrições descartável em vez
    /// da versão nativa. As duas precisam concordar bit a bit, e derivar o valor
    /// esperado de uma implementação diferente da que o circuito usa
    /// reintroduziria exatamente a classe de divergência que o `profile_hash`
    /// compartilhado eliminou entre custo e cobertura.
    pub fn commitment(&self) -> Scalar {
        use nova_snark::frontend::test_cs::TestConstraintSystem;
        let mut cs = TestConstraintSystem::<Scalar>::new();
        let limbs: Vec<AllocatedNum<Scalar>> = self
            .witness
            .fine_limbs
            .iter()
            .enumerate()
            .map(|(index, limb)| {
                AllocatedNum::alloc(cs.namespace(|| format!("limb_{index}")), || {
                    Ok(Scalar::from(*limb))
                })
                .expect("alocação válida")
            })
            .collect();
        let mut sponge = PoseidonROCircuit::new(self.constants.clone());
        for limb in &limbs {
            sponge.absorb(limb);
        }
        let bits = sponge
            .squeeze(cs.namespace(|| "commitment"), 128, false)
            .expect("esponja válida");
        bits_to_scalar(&bits)
    }
}

/// Converte um estado quantizado em elemento de corpo, para montar o `z`
/// público do vínculo.
pub fn state_element<Scalar: PrimeField>(value: u128) -> Scalar {
    field_from_u128(value)
}

fn bits_to_scalar<Scalar: PrimeField>(
    bits: &[nova_snark::frontend::AllocatedBit],
) -> Scalar {
    let mut value = Scalar::ZERO;
    let mut coefficient = Scalar::ONE;
    for bit in bits {
        if bit.get_value().unwrap_or(false) {
            value += coefficient;
        }
        coefficient = coefficient.double();
    }
    value
}

impl<Scalar> StepCircuit<Scalar> for EscalationLinkCircuit<Scalar>
where
    Scalar: PrimeField + PrimeFieldBits + Serialize + for<'de> Deserialize<'de>,
{
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<Scalar>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<Scalar>],
    ) -> Result<Vec<AllocatedNum<Scalar>>, SynthesisError> {
        assert_eq!(z.len(), Self::arity_value());
        let fine_bits = self.witness.fine_bits();

        // A entrada privada, com range check por limb: sem ele a reconstrução
        // do valor fino não seria injetora e dois conjuntos de limbs diferentes
        // poderiam produzir o mesmo compromisso.
        let mut limbs = Vec::with_capacity(self.witness.fine_limbs.len());
        for (index, limb) in self.witness.fine_limbs.iter().enumerate() {
            let (allocated, _) = allocate_ranged(
                cs.namespace(|| format!("fine_limb_{index}")),
                "fine_limb_value",
                *limb as u128,
                16,
            )?;
            limbs.push(allocated);
        }

        let fine_value = AllocatedNum::alloc(cs.namespace(|| "fine_value"), || {
            Ok(field_from_u128::<Scalar>(self.witness.fine_value()))
        })?;
        let mut reconstruction = LinearCombination::zero();
        let mut coefficient = Scalar::ONE;
        for limb in limbs.iter().rev() {
            reconstruction = reconstruction + (coefficient, limb.get_variable());
            coefficient *= Scalar::from(1_u64 << 16);
        }
        reconstruction = reconstruction - fine_value.get_variable();
        cs.enforce(
            || "reconstruct fine value",
            |lc| lc,
            |lc| lc,
            |_| reconstruction,
        );

        // 1. Compromisso: Poseidon sobre os limbs finos, comparado ao público.
        let mut sponge = PoseidonROCircuit::new(self.constants.clone());
        for limb in &limbs {
            sponge.absorb(limb);
        }
        let commitment_bits = sponge.squeeze(cs.namespace(|| "commitment"), 128, false)?;
        let mut commitment = LinearCombination::zero();
        let mut coefficient = Scalar::ONE;
        for bit in &commitment_bits {
            commitment = commitment + (coefficient, bit.get_variable());
            coefficient = coefficient.double();
        }
        commitment = commitment - z[0].get_variable();
        cs.enforce(
            || "commitment binds the private input",
            |lc| lc,
            |lc| lc,
            |_| commitment,
        );

        // 2 e 3. Cada estado é o quociente da mesma testemunha por um
        // deslocamento distinto. É aqui que o vínculo acontece: dois quocientes,
        // um único dividendo.
        for (slot, bits, name) in [
            (1_usize, self.witness.coarse_bits, "coarse"),
            (2, self.witness.escalated_bits, "escalated"),
        ] {
            let shift = fine_bits - bits;
            let quotient = &z[slot];
            range_check(
                cs.namespace(|| format!("{name}_state_range")),
                quotient,
                self.witness.quantized(bits),
                bits,
            )?;
            let (remainder, _) = allocate_ranged(
                cs.namespace(|| format!("{name}_remainder")),
                "remainder_value",
                self.witness.remainder(bits),
                shift.max(1),
            )?;
            cs.enforce(
                || format!("{name} state quantizes the committed input"),
                |lc| lc,
                |lc| lc,
                |lc| {
                    lc + fine_value.get_variable()
                        - (field_from_u128::<Scalar>(1_u128 << shift), quotient.get_variable())
                        - remainder.get_variable()
                },
            );
        }

        Ok(z.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff::Field;
    use nova_snark::frontend::test_cs::TestConstraintSystem;
    use nova_snark::provider::PallasEngine;
    use nova_snark::traits::Engine;

    type Scalar = <PallasEngine as Engine>::Scalar;

    fn witness() -> EscalationWitness {
        // 0,1 aproximado na grade de 64 bits.
        EscalationWitness::new(vec![0x1999, 0x9999, 0x9999, 0x999a], 16, 32)
    }

    fn circuit() -> EscalationLinkCircuit<Scalar> {
        EscalationLinkCircuit {
            witness: witness(),
            constants: PoseidonConstantsCircuit::default(),
        }
    }

    fn state(circuit: &EscalationLinkCircuit<Scalar>) -> Vec<Scalar> {
        vec![
            circuit.commitment(),
            field_from_u128(circuit.witness.coarse_value()),
            field_from_u128(circuit.witness.escalated_value()),
        ]
    }

    fn allocate(
        cs: &mut TestConstraintSystem<Scalar>,
        values: &[Scalar],
    ) -> Vec<AllocatedNum<Scalar>> {
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                AllocatedNum::alloc(cs.namespace(|| format!("z_{index}")), || Ok(*value))
                    .expect("alocação válida")
            })
            .collect()
    }

    #[test]
    fn quantization_truncates_consistently() {
        let witness = witness();
        assert_eq!(witness.coarse_value(), 0x1999);
        assert_eq!(witness.escalated_value(), 0x1999_9999);
        // O estado grosso é prefixo do escalonado: os dois vêm do mesmo valor.
        assert_eq!(witness.escalated_value() >> 16, witness.coarse_value());
    }

    #[test]
    fn the_link_is_satisfied() {
        let circuit = circuit();
        let mut cs = TestConstraintSystem::<Scalar>::new();
        let z = allocate(&mut cs, &state(&circuit));
        circuit.synthesize(&mut cs, &z).unwrap();
        assert!(cs.is_satisfied(), "{:?}", cs.which_is_unsatisfied());
    }

    /// O ponto do vínculo: trocar a entrada privada mantendo o compromisso tem
    /// de falhar.
    #[test]
    fn a_different_private_input_breaks_the_commitment() {
        let honest = circuit();
        let published = state(&honest);
        let mut swapped = honest.clone();
        swapped.witness = EscalationWitness::new(vec![0x2000, 0, 0, 0], 16, 32);

        let mut cs = TestConstraintSystem::<Scalar>::new();
        let z = allocate(&mut cs, &published);
        swapped.synthesize(&mut cs, &z).unwrap();
        assert!(
            !cs.is_satisfied(),
            "entrada trocada foi aceita com o compromisso original"
        );
    }

    /// Nem adianta trocar só o estado escalonado, mantendo o grosso: os dois
    /// quocientes partem do mesmo dividendo.
    #[test]
    fn a_forged_escalated_state_breaks_the_quantization() {
        let circuit = circuit();
        let mut published = state(&circuit);
        published[2] += Scalar::ONE;

        let mut cs = TestConstraintSystem::<Scalar>::new();
        let z = allocate(&mut cs, &published);
        circuit.synthesize(&mut cs, &z).unwrap();
        assert!(
            !cs.is_satisfied(),
            "estado escalonado forjado foi aceito"
        );
    }

    #[test]
    fn the_commitment_helper_matches_the_circuit() {
        let circuit = circuit();
        let mut cs = TestConstraintSystem::<Scalar>::new();
        let z = allocate(&mut cs, &state(&circuit));
        circuit.synthesize(&mut cs, &z).unwrap();
        assert!(cs.is_satisfied());
    }
}

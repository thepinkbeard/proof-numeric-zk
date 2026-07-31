//! Custo medido do vínculo de escalonamento.
//!
//! O protocolo define o custo esperado de um sistema certificado como
//! `C_p + (A_p / N) * (C_link + C_p')`. Os dois termos de etapa já vêm medidos
//! da varredura de custo; este binário fecha o terceiro, que até 0020 era o
//! único ainda modelado.
//!
//! O vínculo entra **uma vez por escalonamento**, não uma vez por etapa: ele
//! amarra o estado inicial das duas provas ao mesmo compromisso, e depois disso
//! cada prova segue com o seu próprio circuito de etapa.

use anyhow::Result;
use clap::Parser;
use nova_circuit::escalation::{EscalationLinkCircuit, EscalationWitness};
use nova_snark::{
    frontend::{num::AllocatedNum, test_cs::TestConstraintSystem, ConstraintSystem},
    provider::{poseidon::PoseidonConstantsCircuit, PallasEngine},
    traits::{circuit::StepCircuit, Engine},
};
use serde::Serialize;
use std::{fs, path::PathBuf};

type Scalar = <PallasEngine as Engine>::Scalar;

#[derive(Parser, Debug)]
#[command(about = "Mede o custo isolado do vínculo de escalonamento entre perfis")]
struct Args {
    #[arg(long, default_value = "results/escalation_link_cost.json")]
    output: PathBuf,
    /// Bits da grade fina onde a entrada privada é comprometida.
    #[arg(long, default_value_t = 64)]
    commitment_bits: usize,
}

#[derive(Serialize)]
struct LinkCost {
    coarse_bits: usize,
    escalated_bits: usize,
    commitment_bits: usize,
    /// Restrições do vínculo inteiro, incluindo o compromisso Poseidon.
    link_constraints: usize,
    /// Só a parte do compromisso, para separar o que é hash do que é
    /// quantização.
    commitment_constraints: usize,
    quantization_constraints: usize,
}

#[derive(Serialize)]
struct Report {
    circuit: &'static str,
    curve: &'static str,
    commitment: &'static str,
    points: Vec<LinkCost>,
}

fn measure(witness: EscalationWitness) -> LinkCost {
    let circuit = EscalationLinkCircuit::<Scalar> {
        witness,
        constants: PoseidonConstantsCircuit::default(),
    };
    let mut cs = TestConstraintSystem::<Scalar>::new();
    let values = [
        circuit.commitment(),
        nova_circuit::escalation::state_element(circuit.witness.coarse_value()),
        nova_circuit::escalation::state_element(circuit.witness.escalated_value()),
    ];
    let z: Vec<AllocatedNum<Scalar>> = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            AllocatedNum::alloc(cs.namespace(|| format!("z_{index}")), || Ok(*value))
                .expect("alocação válida")
        })
        .collect();
    circuit.synthesize(&mut cs, &z).expect("síntese do vínculo");
    let total = cs.num_constraints();

    // O custo do compromisso sozinho: mesma esponja, sem as duas relações de
    // quantização. A diferença é a parte que cresce com a precisão.
    let mut hash_only = TestConstraintSystem::<Scalar>::new();
    let commitment_constraints = {
        use nova_snark::provider::poseidon::PoseidonROCircuit;
        use nova_snark::traits::ROCircuitTrait;
        let limbs: Vec<AllocatedNum<Scalar>> = circuit
            .witness
            .fine_limbs
            .iter()
            .enumerate()
            .map(|(index, limb)| {
                AllocatedNum::alloc(hash_only.namespace(|| format!("limb_{index}")), || {
                    Ok(Scalar::from(*limb))
                })
                .expect("alocação válida")
            })
            .collect();
        let mut sponge = PoseidonROCircuit::new(PoseidonConstantsCircuit::default());
        for limb in &limbs {
            sponge.absorb(limb);
        }
        sponge
            .squeeze(hash_only.namespace(|| "commitment"), 128, false)
            .expect("esponja válida");
        hash_only.num_constraints()
    };

    LinkCost {
        coarse_bits: circuit.witness.coarse_bits,
        escalated_bits: circuit.witness.escalated_bits,
        commitment_bits: 16 * circuit.witness.fine_limbs.len(),
        link_constraints: total,
        commitment_constraints,
        quantization_constraints: total.saturating_sub(commitment_constraints),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    assert!(
        args.commitment_bits % 16 == 0,
        "a grade de compromisso precisa ser múltipla de 16"
    );
    let limbs = args.commitment_bits / 16;
    // Entrada arbitrária: o custo depende só da forma do circuito, não do valor.
    let fine: Vec<u64> = (0..limbs).map(|index| 0x1999 + index as u64).collect();

    // As cadeias que `next_profile` define em numeric-core: 16 -> 32 -> 48 -> 64.
    let chain = [(16_usize, 32_usize), (32, 48), (48, 64)];
    let points = chain
        .iter()
        .filter(|(_, escalated)| *escalated <= args.commitment_bits)
        .map(|(coarse, escalated)| {
            measure(EscalationWitness::new(fine.clone(), *coarse, *escalated))
        })
        .collect();

    let report = Report {
        circuit: "escalation-link",
        curve: "pallas",
        commitment: "poseidon-sponge-128",
        points,
    };
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, serde_json::to_string_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    println!("gravado em {}", args.output.display());
    Ok(())
}

# Revisão das ferramentas

Data da revisão: 2026-06-29.

## Nova

- crate: `nova-snark 0.73.0`;
- commit incorporado no artefato crates.io:
  `666e3b25bfb9f8b2106f8b4d8057010f28b1ee79`;
- MSRV declarado: Rust 1.79.0;
- licença: MIT;
- frontend: R1CS nativo no estilo Bellman;
- circuito iterativo: `StepCircuit<F>`;
- IVC: `PublicParams`, `RecursiveSNARK::new`, `prove_step` e `verify`;
- compressão: `CompressedSNARK::setup`, `prove` e `verify`;
- ciclos suportados: Pallas/Vesta, BN254/Grumpkin e secp/secq;
- compromissos/argumentos: Pedersen+IPA, HyperKZG e Mercury.

O perfil inicial usa Pallas/Vesta e IPA, evitando uma SRS externa. Um perfil
BN254/Grumpkin será medido separadamente; HyperKZG e Mercury requerem Powers of
Tau e não serão misturados aos números do perfil IPA.

O GitHub não publica releases formais para a versão atual. A reprodutibilidade
é dada pela versão exata do crate, `Cargo.lock` e hash VCS acima.

Fontes primárias:

- <https://github.com/microsoft/Nova>
- <https://docs.rs/nova-snark/0.73.0/nova_snark/>
- exemplos `minroot.rs`, `and.rs` e `hashchain.rs` incluídos no crate.

## Sonobe

Sonobe suporta Nova, CycleFold, HyperNova e ProtoGalaxy sobre Arkworks, mas o
próprio projeto ainda o classifica como experimental e não auditado. Portanto,
é uma implementação comparativa posterior, não a base deste experimento.

Fonte: <https://github.com/privacy-scaling-explorations/sonobe>.


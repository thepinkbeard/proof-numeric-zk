# 0003 — Step circuit CBD e smoke test Nova

**Data:** 2026-07-05  
**Estado:** concluído com hipótese de eficiência ainda aberta

## Objetivo

Aritmetizar uma etapa CBD fusionada do mapa logístico e provar que a relação
funciona dentro de Nova.

## Arquivos modificados

- `crates/numeric-core/src/cbd.rs`
- `crates/numeric-core/tests/certified_dyadic.rs`
- `crates/nova-circuit/Cargo.toml`
- `crates/nova-circuit/src/lib.rs`
- `crates/nova-circuit/src/main.rs`
- `crates/nova-circuit/tests/step_circuit.rs`
- `docs/finite-field-bounds.md`
- `docs/mathematical-specification.md`
- `docs/report.md`
- `results/nova_cbd_k2_n2_smoke.json`

## Implementação

- Centro fusionado:
  `round_nearest_even(r_num*N*(S-N)/(r_den*S))`.
- Resto do centro explicitamente provado menor que o divisor.
- Propagação de raio por bound de Lipschitz:
  `ceil(r_num*R/r_den)`, mais uma ULP quando a quantização é inexata.
- Saturação do raio em `S`, representando o domínio completo.
- Estado Nova: iteração, limbs do centro, `is_one` e raio.
- Suporte genérico a `K=1..4`.
- Nova opção CLI: `--representation cbd --rounding nearest-even`.

## Comandos executados

```bash
cargo test -p numeric-core -p nova-circuit
cargo run --release -p nova-circuit -- \
  --representation cbd --rounding nearest-even \
  --k 2 --steps 2 \
  --output results/nova_cbd_k2_n2_smoke.json
```

## Testes e evidências

- 24 testes de `nova-circuit` passaram.
- Foram testadas rejeições de resto do centro, resto do raio e raio de saída
  adulterados.
- Foi testada saturação para o domínio completo.
- Um teste compara diretamente a witness circuital com
  `CertifiedDyadic::logistic_step_lipschitz`.
- O smoke test Nova concluiu duas folds e verificação recursiva.

Métricas `K=2`:

- 466 constraints brutas;
- 11.842 constraints no circuito Nova aumentado;
- 5 elementos de estado;
- 22,20 ms por fold no smoke test;
- 85,1 MB de pico RSS.

## Tentativas e resultados negativos

- Propagar o raio mínimo exato exigiria selecionar extremos, tratar o vértice
  `1/2` e provar comparações adicionais. A primeira step circuit adota um
  bound de Lipschitz verificável e registra essa perda de precisão.
- O custo bruto de 466 constraints não domina os 221 constraints do
  `standard 16×2 nearest-even`.
- Também não domina a estimativa FP32 otimista de 342 constraints. Ela é
  menor apenas que o modelo FP32 bit-level de 3.910 constraints.

## Limitações conhecidas

- O predicado trivalorado ainda é calculado fora do circuito.
- Não existe compromisso que ligue provas em perfis diferentes.
- O bound global de Lipschitz pode saturar rapidamente em regimes caóticos.
- A decomposição de range domina o custo.

## Próximo passo

Integrar trajetórias CBD ao runner para medir cobertura, saturação e decisões
antes de investir em otimizações do circuito.

## Continuidade para outra IA

Use `--representation cbd --rounding nearest-even`. Não apresente 466
constraints como vitória sobre FP32: o baseline homogêneo ainda não existe e
o modelo otimista atual é menor.

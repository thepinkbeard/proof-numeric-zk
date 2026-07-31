# 0014 — Circuito CBD sequencial

**Data:** 2026-07-05  
**Estado:** concluído

## Objetivo

Provar no mesmo R1CS/Nova as duas quantizações, os dois bounds de raio e a
decisão trivalorada da semântica sequencial.

## Arquivos modificados

- `crates/nova-circuit/src/lib.rs`
- `crates/nova-circuit/src/main.rs`
- `crates/nova-circuit/tests/step_circuit.rs`
- `scripts/run_nova_benchmarks.sh`

## Decisões e hipóteses

- `CbdSequentialStepWitness<K>` retém quocientes, restos, bits RNE, raios
  intermediários, saturações, saída e decisão.
- `CbdSequentialLogisticStepCircuit<K>` mantém o estado público `K+4`.
- A opção CLI é `--representation cbd-sequential`.
- Não foram usados lookups, FMA ou multiplicação intervalar independente.

## Comandos executados

```bash
cargo test -p nova-circuit --test step_circuit cbd_sequential
cargo run --release -p nova-circuit -- \
  --steps 2 --k 2 --rounding nearest-even \
  --representation cbd-sequential
```

## Testes, métricas e evidências

- 256 witnesses aleatórios coincidiram com a referência; 64 foram
  sintetizados em R1CS.
- Resto, RNE, raio intermediário, resto final, raio final e decisão adulterados
  foram rejeitados.
- A forma R1CS é igual para zero, `0,1`, `0,5` e um.
- Smoke IVC de duas etapas concluiu setup, folding e verificação.

| Bits | Constraints | Range/modeladas | Estado | Nova aumentado |
|---:|---:|---:|---:|---:|
| 16 | 472 | 446 | 5 FE | 11.848 |
| 32 | 782 | 753 | 6 FE | 12.159 |
| 48 | 1.092 | 1.060 | 7 FE | 12.470 |
| 64 | 1.402 | 1.367 | 8 FE | 12.781 |

## Tentativas malsucedidas

Não houve caminho circuital abandonado. Testes adversariais produzem mensagens
de constraint insatisfeita esperadas e não são falhas da suíte.

## Limitações conhecidas

- A instrumentação de categorias além de range checks é aproximada.
- O circuito restringe `r` ao racional público configurado.
- Compromisso entre perfis ainda não faz parte do estado.

## Próximo passo

Medir cobertura e custo contra CBD fusionado e FP32 em protocolo homogêneo.

## Continuidade para outra IA

Use `CbdSequentialStepWitness::compute` como única fonte de witness; não
duplique a recorrência no runner.

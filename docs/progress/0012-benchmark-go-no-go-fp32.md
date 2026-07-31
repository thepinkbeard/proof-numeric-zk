# 0012 — Benchmark FP32 homogêneo e decisão go/no-go

**Data:** 2026-07-05  
**Estado:** concluído; decisão **go**

## Objetivo

Medir o FP32 restrito exato no mesmo Nova/R1CS e decidir se a hipótese de custo
do CBD merece continuar.

## Arquivos modificados

- `crates/nova-circuit/src/main.rs`
- `scripts/run_nova_benchmarks.sh`
- `results/nova_fp32_exact_n10_compressed*.json`
- `results/nova_cbd_local_decision_k2_n10_compressed*_current.json`
- `results/fp32_cbd_go_no_go.json`
- `docs/report.md`
- `docs/reproducibility.md`
- `docs/thesis-execution-plan.md`
- `README.md`
- `docs/progress/README.md`

## Decisões e hipóteses

- A comparação usa Nova 0.73, Pallas/Vesta, Pedersen+IPA, 10 folds, `x0=0,1`,
  `r=3,7` e decisão `x>=0,5`.
- Constraints são determinísticas; fold, compressão, verificação e memória
  usam a média de três repetições.
- A decisão é `go` porque o FP32 completo excedeu 709 constraints.
- A alegação permanece restrita ao perfil positivo-normal e reconhece que CBD
  usa uma quantização fusionada, enquanto FP32 preserva três operações.

## Comandos executados

```bash
cargo run --release -p nova-circuit -- --steps 2 --k 2 \
  --rounding nearest-even --representation fp32
cargo run --release -p nova-circuit -- --steps 10 --k 2 \
  --rounding nearest-even --representation fp32 --compress
target/release/nova-circuit --steps 10 --k 2 \
  --rounding nearest-even --representation cbd --compress
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

As duas últimas medições foram repetidas três vezes; os JSONs brutos estão em
`results/`.

## Testes, métricas e evidências

| Métrica | CBD-32 | FP32 exato |
|---|---:|---:|
| Constraints brutas | 709 | 1.721 |
| Range checks | 689 | 1.551 |
| Nova aumentado | 12.086 | 13.097 |
| Estado | 6 FE | 5 FE |
| Prover total médio | 0,3148 s | 0,3220 s |
| Compressão média | 0,9654 s | 1,0519 s |
| Verificação comprimida média | 22,16 ms | 22,13 ms |
| Prova | 10.752 B | 10.720 B |
| Pico RSS médio | 158,1 MB | 161,5 MB |

- FP32/CBD = 2,43× em constraints brutas e 1,084× no circuito Nova aumentado.
- 1.551/1.721 = 90,1% das constraints FP32 são range checks.
- Primitivas isoladas: multiplicação 474; `1-x` 924.
- O artefato consolidado é `results/fp32_cbd_go_no_go.json`.

## Tentativas malsucedidas

O modelo otimista anterior de 342 constraints subestimou em 5,03× o gadget
medido. Ele permanece apenas como registro de hipótese, não como baseline.

## Limitações conhecidas

- Os tempos são smoke benchmarks de três repetições.
- A prova FP32 menor decorre do estado com um elemento a menos; tamanho de
  prova e tempo não acompanham linearmente as constraints brutas devido ao
  overhead Nova.
- Não há comparação com IEEE 754 geral nem com a mesma ordem de quantização.
- Berkeley SoftFloat permanece pendente conforme o registro 0011.

## Próximo passo

Vincular os perfis CBD 16→32→48→64 ao mesmo compromisso de entrada e levar o
workload ECEF para circuito. Esse marco deve medir custo por decisão definitiva
e taxa de escalonamento, não apenas constraints por operação.

## Continuidade para outra IA

Comece pelo compromisso de entrada entre perfis. Preserve este baseline e não
declare vitória universal sobre IEEE 754; a conclusão válida é 709 contra
1.721 no subconjunto e workload medidos.

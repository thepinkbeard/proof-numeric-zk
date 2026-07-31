# 0015 — Ablação de fusão e representação

**Data:** 2026-07-05  
**Estado:** concluído; vantagem não depende apenas da fusão

## Objetivo

Comparar CBD fusionado, CBD sequencial e FP32 sequencial no mesmo backend,
incluindo custo e cobertura certificada.

## Arquivos modificados

- `crates/experiment-runner/src/main.rs`
- `scripts/analyze_cbd_ablation.py`
- `data/trajectory_ablation_r*_n1000.*`
- `results/nova_cbd_sequential_*.json`
- `results/cbd_fusion_ablation.json`
- `docs/report.md`
- `docs/reproducibility.md`
- `docs/thesis-execution-plan.md`
- `README.md`
- `docs/progress/README.md`

## Decisões e hipóteses

- Trajetórias usam 1.000 passos, três limiares e perfis 16/32/48/64.
- Nova usa 10 folds, `x0=0,1`, `r=3,7`, compressão e três repetições.
- A conclusão causal depende das constraints determinísticas; tempos são
  smoke benchmarks.

## Comandos executados

```bash
target/release/experiment-runner --steps 1000 --r 3.7 \
  --output data/trajectory_ablation_r3_7_n1000.csv
target/release/nova-circuit --steps 10 --k 2 \
  --rounding nearest-even --representation cbd-sequential --compress
python3 scripts/analyze_cbd_ablation.py
```

Os comandos de trajetória foram repetidos para `r={3,5;3,7;3,9;4,0}` e o
benchmark Nova três vezes.

## Testes, métricas e evidências

| Variante K=2 | Step constraints | Nova aumentado | Prover médio | Compressão |
|---|---:|---:|---:|---:|
| CBD fusionado | 709 | 12.086 | 0,3148 s | 0,9654 s |
| CBD sequencial | 782 | 12.159 | 0,3244 s | 1,0178 s |
| FP32 sequencial | 1.721 | 13.097 | 0,3220 s | 1,0519 s |

- CBD sequencial custa 10,3% mais que o fusionado, mas apenas 45,4% do FP32.
- Até CBD-64 sequencial, com 1.402 constraints, permanece abaixo do FP32.
- Em CBD-32 e três limiares, o sequencial certificou 3.359 decisões contra
  3.377 do fusionado; redução de 0,53%.
- Nenhuma decisão certificada incorreta ocorreu em 96.096 avaliações de
  formato/estado/limiar das duas variantes CBD principais.
- O artefato consolidado é `results/cbd_fusion_ablation.json`.

## Tentativas malsucedidas

A ordenação ponto a ponto de largura foi falsificada no registro 0013. A
comparação foi corrigida para cobertura, inclusão e soundness sem alterar os
dados.

## Limitações conhecidas

- FP32 continua restrito a positivos normais e `+0`.
- CBD e FP32 não possuem a mesma semântica de fidelidade.
- O overhead Nova torna os tempos próximos apesar da diferença bruta.
- Três repetições não sustentam ordenação temporal forte.

## Próximo passo

Vincular os perfis 16→32→48→64 ao mesmo compromisso de entrada e implementar o
workload ECEF em circuito, medindo custo por decisão definitiva e escalonamento.

## Continuidade para outra IA

A alegação segura agora é: no domínio medido, CBD-32 sequencial custa 782
constraints contra 1.721 do FP32 restrito; a vantagem bruta não vem apenas da
fusão. Não generalize para IEEE 754 completo.

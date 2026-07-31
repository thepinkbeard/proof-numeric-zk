# 0004 — Avaliação do bound global CBD

**Data:** 2026-07-05  
**Estado:** concluído; resultado negativo preservado

## Objetivo

Medir a cobertura de decisões da recorrência circuital
`R'=ceil(rR)+quantização` antes de investir em sua otimização.

## Arquivos modificados

- `crates/experiment-runner/src/main.rs`
- `data/trajectory_cbd_extended_r3_5_n1000.csv`
- `data/trajectory_cbd_extended_r3_5_n1000.summary.json`
- `data/trajectory_cbd_extended_r3_7_n1000.csv`
- `data/trajectory_cbd_extended_r3_7_n1000.summary.json`
- `data/trajectory_cbd_extended_r3_9_n1000.csv`
- `data/trajectory_cbd_extended_r3_9_n1000.summary.json`
- `data/trajectory_cbd_extended_r4_0_n1000.csv`
- `data/trajectory_cbd_extended_r4_0_n1000.summary.json`
- `docs/report.md`

## Implementação

O runner agora registra intervalo certificado, raio em ULPs, decisão
trivalorada para 0,25/0,50/0,75 e decisões certificadas incorretas. Foram
adicionados perfis CBD de 16, 32, 48 e 64 bits.

## Comandos executados

```bash
cargo test -p experiment-runner
for r in 3.5 3.7 3.9 4.0; do
  cargo run --release -q -p experiment-runner -- \
    --steps 1000 --r "$r" \
    --output "data/trajectory_cbd_extended_${r}_n1000.csv"
done
```

## Resultados

Não houve decisão certificada incorreta em nenhum perfil ou regime.

Entretanto, a cobertura foi muito baixa:

- `r=3.5`, CBD-32, limiar 0,50: 17 definitivas e 984 indeterminadas;
- `r=3.7`, CBD-64, limiar 0,50: 33 definitivas e 968 indeterminadas;
- `r=3.9`, CBD-64, limiar 0,50: 31 definitivas e 970 indeterminadas;
- `r=4.0`, CBD-64, limiar 0,50: 32 definitivas e 969 indeterminadas.

O bound global perde quase toda disponibilidade mesmo quando o erro do centro
em `r=3.5` permanece pequeno.

## Tentativa malsucedida

A hipótese de que um bound global de Lipschitz seria uma primeira semântica
circuital suficientemente útil foi rejeitada. Ela é sound, simples e barata,
mas excessivamente conservadora para iteração longa.

## Decisão

Manter a variante global como baseline negativo. Implementar o bound local:

```text
ceil(r * (R*|S-2N| + R²) / S) + quantization_error
```

Esse bound vem da expansão exata de `x(1-x)` ao redor do centro e deve crescer
de acordo com a derivada local, não sempre pelo pior caso global.

## Limitações conhecidas

- As métricas contam o estado inicial, totalizando 1.001 decisões.
- A decisão ainda não é constrained dentro do circuito.
- Cobertura baixa impede qualquer alegação prática da variante global.

## Próximo passo

Implementar e comparar o bound local off-circuit. Só aritmetizá-lo se houver
ganho material de cobertura.

## Continuidade para outra IA

Não remova a variante global nem seus dados. Ela é uma ablação importante e
um resultado negativo que justifica o custo adicional do bound local.

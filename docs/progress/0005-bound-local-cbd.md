# 0005 — Bound local CBD

**Data:** 2026-07-05  
**Estado:** concluído

## Objetivo

Substituir o bound global excessivamente conservador por uma recorrência local
sound e aritmetizável.

## Arquivos modificados

- `crates/numeric-core/src/cbd.rs`
- `crates/numeric-core/tests/certified_dyadic.rs`
- `crates/experiment-runner/src/main.rs`
- `crates/nova-circuit/src/lib.rs`
- `crates/nova-circuit/src/main.rs`
- `crates/nova-circuit/tests/step_circuit.rs`
- `data/trajectory_cbd_bounds_*_n1000.csv`
- `data/trajectory_cbd_bounds_*_n1000.summary.json`
- `results/nova_cbd_local_k2_n2_smoke.json`
- `docs/report.md`

## Semântica

Para centro `N/S` e raio `R/S`, a expansão exata fornece:

```text
|f(N+d)-f(N)| <= r*(R*|S-2N| + R²)/S
```

O novo raio é o teto desse valor, mais uma ULP quando a quantização do centro
é inexata. Ele satura em `S`.

## Circuito

O circuito prova:

- o lado de `N` em relação a `S/2`;
- `|S-2N|`;
- `R*|S-2N|` e `R²`;
- divisão inteira canônica do bound;
- quantização nearest-even do centro;
- saturação do raio.

As opções CLI são:

- `--representation cbd`: bound local;
- `--representation cbd-global`: ablação global.

## Comandos executados

```bash
cargo test -p numeric-core -p experiment-runner -p nova-circuit
cargo run --release -q -p nova-circuit -- \
  --representation cbd --rounding nearest-even \
  --k 2 --steps 2 \
  --output results/nova_cbd_local_k2_n2_smoke.json
```

Também foram executadas trajetórias de 1.000 passos para
`r={3.5,3.7,3.9,4.0}`.

## Resultados

Cobertura no limiar 0,50:

| Regime | Perfil | Definitivas | Indeterminadas | Incorretas |
|---|---|---:|---:|---:|
| 3.5 | CBD-32 | 1.001 | 0 | 0 |
| 3.7 | CBD-64 | 122 | 879 | 0 |
| 3.9 | CBD-64 | 85 | 916 | 0 |
| 4.0 | CBD-64 | 60 | 941 | 0 |

Circuito local `K=2`:

- 557 constraints brutas;
- 11.933 constraints Nova aumentadas;
- 5 elementos de estado;
- 21,54 ms/fold no smoke test;
- 85,7 MB de pico RSS.

## Tentativas malsucedidas

- Um teste com 100 passos de referência racional exata foi interrompido:
  o tamanho da fração cresce exponencialmente. O teste composicional foi
  reduzido para 12 passos; horizontes longos continuam usando MPFR.

## Limitações conhecidas

- O predicado trivalorado ainda não é constrained.
- 557 constraints não dominam a estimativa FP32 otimista de 342.
- Em caos, a decisão torna-se indeterminada após o horizonte previsível; isso
  é comportamento esperado, não falha de soundness.
- A instrumentação de categorias de constraints é aproximada.

## Próximo passo

Adicionar decisão trivalorada constrained e compromisso de entrada/perfil ao
estado público.

## Continuidade para outra IA

Use o bound local como CBD principal. Preserve `cbd-global` apenas para
ablação e mantenha referências longas em MPFR, nunca em racional exato.

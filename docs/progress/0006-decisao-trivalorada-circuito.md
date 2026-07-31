# 0006 — Decisão trivalorada no circuito

**Data:** 2026-07-05  
**Estado:** concluído

## Objetivo

Fazer o circuito provar, e não apenas reportar fora dele, a decisão
`x>=0,5`.

## Arquivos modificados

- `crates/nova-circuit/src/lib.rs`
- `crates/nova-circuit/src/main.rs`
- `crates/nova-circuit/tests/step_circuit.rs`
- `results/nova_cbd_local_decision_k2_n2_smoke.json`
- `docs/report.md`

## Implementação

A decisão usa código:

- `0`: false;
- `1`: true;
- `2`: indeterminate.

Bits one-hot e diferenças não negativas provam simultaneamente:

```text
true          <=> center-radius >= S/2
false         <=> center+radius <  S/2
indeterminate <=> nenhuma das anteriores
```

O código é incluído no estado Nova. O predicado inicial é fixo em `0,5`; os
demais limiares permanecem apenas no runner.

## Comandos executados

```bash
cargo test -p nova-circuit
cargo run --release -q -p nova-circuit -- \
  --representation cbd --rounding nearest-even \
  --k 2 --steps 2 \
  --output results/nova_cbd_local_decision_k2_n2_smoke.json
```

## Resultados

- 26 testes de `nova-circuit` passaram.
- Uma witness com decisão adulterada é rejeitada.
- 709 constraints brutas;
- 12.086 constraints Nova aumentadas;
- 6 elementos de estado;
- 21,49 ms/fold no smoke test;
- 86,9 MB de pico RSS.

## Tentativa malsucedida

O primeiro teste de decisão adulterada causou underflow no cálculo host-side
das witnesses auxiliares. As diferenças foram alteradas para subtrações
saturadas; valores fraudulentos agora chegam ao constraint system e são
rejeitados, em vez de causar panic.

## Limitações conhecidas

- Apenas o limiar 0,5 está constrained.
- O custo de decisão acrescentou 152 constraints à step local.
- O perfil e o compromisso de entrada ainda não estão vinculados
  criptograficamente entre provas de diferentes precisões.

## Próximo passo

Construir o baseline FP32 positivo-normal no mesmo backend. A vinculação entre
perfis exigirá um hash/commitment gadget e não deve ser simulada por um campo
público sem ligação à entrada.

## Continuidade para outra IA

Use o arquivo de métricas com sufixo `local_decision`. Não compare 557
constraints de transição numérica com um baseline que inclua decisão; o número
comparável agora é 709.

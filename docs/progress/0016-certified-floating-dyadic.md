# 0016 — Certified Floating Dyadic

Data: 2026-07-13. Estado: implementação v1 concluída; decisão científica
parcial, com resultado negativo explícito em caos.

## Objetivo e arquivos

Implementar uma representação de forma IEEE que certifique inclusão e decisões
em IVC, completar a adição FP32 exata e medir custo versus cobertura.

- `crates/numeric-core/src/cfd.rs`: perfil, valor, RNE racional e operações;
- `crates/nova-circuit/src/cfd.rs`: relações R1CS sequencial, fusionada e afim;
- `crates/nova-circuit/src/fp32.rs`: adição FP32 e recorrência afim exatas;
- `crates/experiment-runner/src/bin/cfd_eval.rs`: avaliação racional/MPFR;
- `results/cfd_workloads.json`: 43 configurações até 1.000 passos;
- `docs/cfd-mathematical-specification.md`: contrato e teorema.

## Decisões e evidência

O centro honesto usa RNE, mas a relação prova enclosure. Corredor, domínio e
fallback `wide` pertencem ao hash do perfil. `wide` é absorvente. CFD não é
apresentada como IEEE 754 compliant. A topologia sequencial é a comparação
principal; fusão é ablação.

| Etapa | CFD-24 | FP32 exato | Redução CFD |
|---|---:|---:|---:|
| logística sequencial | 1.003 | 1.721 | 41,7% |
| afim estável | 846 | 1.418 | 40,3% |
| logística fusionada (ablação) | 777 | — | — |

Não houve decisão definitiva incorreta nem violação de inclusão nas 43
configurações. Em `r=3.5`, p16/p24/p32 mantêm as 1.001 decisões em `t=0.5`.
Em regime caótico, conforme perfil e topologia, `wide` surge aproximadamente
entre os passos 13 e 41. Aumentar o corredor de 9 para 17 lanes não resolve o
limite: o raio dinâmico, não apenas o expoente, é o gargalo.

## Testes e comandos

```bash
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p experiment-runner --bin cfd-eval
cargo run --release -p nova-circuit -- --steps 10 --k 1 \
  --rounding nearest-even --representation cfd
```

Os testes cobrem propriedades racionais, empates RNE, carry, mudança de
expoente, corredor, zero, saturação, decisões e witnesses adulteradas.

## Tentativas malsucedidas e limitações

O corredor inicial de 17 lanes custou 1.246 constraints e falhou o alvo de 30%
versus FP32. O corredor de 9 lanes reduziu o custo para 1.003, mas não elimina
a perda de disponibilidade em caos. Berkeley TestFloat/SoftFloat, enumeração
reduzida exaustiva, custo circuital do vínculo `next_profile` e inovações
públicas variáveis permanecem pendentes.

## Veredito e continuidade

Soundness e redução de 30% foram atingidos nos dois workloads. A não-dominância
contra toda a fronteira CBD/fixed-point ainda não está demonstrada: no mapa
caótico, CBD-32 pode oferecer mais decisões por custo menor. O estado correto é
contribuição técnica promissora, mas não `go` científico integral.

O próximo experimento deve comparar custo por decisão definitiva no workload
afim com inovações públicas variadas e estudar raios relativos/logarítmicos.
Não ajuste domínio ou corredor retrospectivamente; preserve como resultado
negativo qualquer perfil barato com perda excessiva de cobertura.

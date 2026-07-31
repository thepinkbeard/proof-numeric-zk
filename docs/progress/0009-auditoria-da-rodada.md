# 0009 — Auditoria da rodada

**Data:** 2026-07-05  
**Estado:** rodada concluída com pendências explícitas

## Objetivo

Verificar todo o workspace, consolidar o que foi realmente implementado e
deixar um handoff sem alegações prematuras.

## Arquivos modificados

- `README.md`
- `docs/report.md`
- `docs/reproducibility.md`
- `docs/progress/README.md`
- código e artefatos listados nos registros 0001–0008.

## Comandos executados

```bash
cargo fmt
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Resultado da verificação

- formatação válida;
- clippy sem warnings;
- 57 testes passaram;
- nenhum teste falhou;
- circuitos adulterados de centro, raio, restos e decisão são rejeitados;
- IVC e compressão CBD concluíram;
- nenhum falso certificado foi observado nos três workloads.

## Entregas concluídas

- plano persistente e diário cronológico;
- especificação CBD;
- implementação racional off-circuit;
- operações, FMA, bounds global/local e escalonamento;
- step circuit CBD `K=1..4`;
- predicado `x>=0,5` constrained;
- Nova IVC e compressão;
- métricas de custo por precisão;
- mapa logístico com cobertura certificada;
- logistic regression UCI Adult;
- proximidade ECEF;
- referência FP32 positiva-normal;
- validação software independente de 10.000 vetores.

## Pendências que impedem declarar o plano integralmente concluído

1. O gadget FP32 R1CS bit-exato não foi implementado.
2. Berkeley SoftFloat não executou em Apple Silicon; o teste está preparado
   para host x86_64.
3. Provas em perfis diferentes ainda não são ligadas por commitment/hash da
   mesma entrada privada.
4. Adult e ECEF têm avaliação off-circuit, mas não step circuits dedicadas.
5. Somente o limiar 0,5 do mapa está constrained.

## Resultados negativos preservados

- O bound global é barato, porém perde quase toda cobertura.
- CBD-32 local com decisão custa 709 constraints, acima do modelo FP32
  otimista não medido de 342.
- A hipótese “CBD é mais barata que FP32 homogêneo” permanece aberta.
- O raio certificado adiciona segurança de decisão, mas não é precisão
  gratuita.

## Limpeza de artefatos

Os quatro CSVs `trajectory_cbd_extended_*` foram removidos por duplicarem as
linhas globais contidas em `trajectory_cbd_bounds_*`. Seus summaries foram
preservados e os registros 0004/0005 mantêm a proveniência.

## Próximo passo recomendado

Tratar o gadget FP32 como próximo marco isolado:

1. multiplicação positiva-normal de 24 bits com normalização e nearest-even;
2. subtração `1-x` com alinhamento variável de expoente;
3. composição da step;
4. TestFloat em host x86_64;
5. comparação de 709 constraints CBD contra o custo realmente medido.

Se esse baseline ficar abaixo de CBD, a contribuição deve ser formulada como
**segurança de decisão e abstention verificável**, não como menor número bruto
de constraints.

## Continuidade para outra IA

Leia este arquivo, depois `0007-referencia-fp32-restrita.md`. Não refaça a CBD:
o gargalo científico atual é o baseline FP32 homogêneo e a comparação
custo–cobertura.

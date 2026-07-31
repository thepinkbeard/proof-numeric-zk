# 0001 — Plano persistente e inventário inicial

**Data:** 2026-07-05  
**Estado:** concluído

## Objetivo

Persistir o plano de execução e registrar o estado do experimento antes da
implementação CBD.

## Arquivos modificados

- `README.md`
- `docs/thesis-execution-plan.md`
- `docs/progress/README.md`
- `docs/progress/0001-inventario-inicial.md`

## Estado encontrado

- Referências racional e MPFR implementadas.
- Formatos FP32, FP64, Q16.16, Q32.32 e limbs até 64 bits avaliados
  off-circuit.
- Step circuits Nova para truncagem, nearest-even, carries e uma variante
  compensada `16×2`.
- IVC e compressão medidos até 1.000 passos.
- `16×2` é semanticamente Q32.32.
- A variante compensada `16×2` usa uma grade fina de 64 bits e muda o ponto de
  quantização; não demonstra precisão adicional gratuita.
- Não há repositório Git detectável neste diretório. A rastreabilidade depende
  dos registros cronológicos e dos próprios artefatos.

## Baseline experimental

- `standard 16×2 nearest-even`: 221 constraints de step.
- `compensated 16×2 nearest-even`: 567 constraints de step.
- modelo FP32 local: estimativa entre 342 e 3.910 constraints, ainda sem gadget
  homogêneo.
- prova comprimida `16×2`: 10.688 bytes.
- custo médio observado: aproximadamente 30 ms por fold.

## Comandos executados

```bash
find proof_numeric/docs -maxdepth 2 -type f -print
cargo test --workspace
```

## Testes e evidências

Antes deste avanço, `cargo test --workspace` passou com:

- 2 testes do runner;
- 18 testes de step circuit;
- 9 testes de `numeric-core`;
- 3 testes da referência;
- doc-tests sem falhas.

Evidências numéricas estão em `results/` e no `docs/report.md`.

## Tentativas malsucedidas

- A extração inicial do paper com `pdftotext` falhou porque o binário não
  estava presente no runtime. A leitura foi concluída com `pdfplumber` e
  renderização via `pdftoppm`.
- O diretório não possui metadados Git, portanto não foi possível registrar
  hash de commit inicial.

## Decisões e hipóteses

- A nova representação será chamada provisoriamente CBD.
- O raio será inicialmente expresso na mesma grade da mantissa.
- A primeira implementação será racional e independente de circuito.
- O mapa logístico permanece microbenchmark inicial, não aplicação final.

## Limitações conhecidas

- Baseline IEEE 754 homogêneo inexistente.
- A representação CBD ainda não foi formalizada em código.
- Workloads de score e ECEF ainda não existem.

## Próximo passo

Formalizar a semântica matemática da CBD e implementar `NumericProfile`,
`CertifiedDyadic` e `Decision3` em `numeric-core`.

## Continuidade para outra IA

Leia `docs/thesis-execution-plan.md`, depois implemente a referência CBD com
aritmética racional exata e testes de inclusão antes de tocar no circuito Nova.

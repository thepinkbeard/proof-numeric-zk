# 0008 — Workloads Adult e proximidade ECEF

**Data:** 2026-07-05  
**Estado:** avaliação off-circuit concluída

## Objetivo

Testar CBD fora do mapa logístico em uma decisão de score e em um predicado
geoespacial próximo de limiar.

## Arquivos modificados

- `scripts/train_adult_logistic.py`
- `crates/numeric-core/src/cbd.rs`
- `crates/numeric-core/tests/certified_dyadic.rs`
- `crates/experiment-runner/src/bin/workload_eval.rs`
- `crates/experiment-runner/Cargo.toml`
- `results/adult_logistic_model.json`
- `results/cbd_workloads.json`
- `results/nova_cbd_local_decision_k{1,2,3,4}_n2.json`
- `results/nova_cbd_local_decision_k2_n10_compressed.json`

## Adult

Foi baixado o Adult diretamente do UCI, DOI `10.24432/C5XW20`. O script usa
seis atributos numéricos, split determinístico e regressão logística treinada
em NumPy.

- 30.162 linhas após remoção de ausentes;
- 80,87% de acurácia no teste completo;
- 1.857 casos retidos, com ênfase na fronteira.

CBD-16 deixou somente um caso indeterminado; CBD-32 ou superior decidiu todos.
Nenhuma decisão definitiva discordou da referência racional.

## ECEF

Foram gerados 2.000 pares de vetores unitários com limiar de distância
equivalente a 100 km; metade fica muito próxima da fronteira.

- CBD-16: 837 definitivas, 1.163 indeterminadas, zero incorretas;
- CBD-32: 1.991 definitivas, 9 indeterminadas, zero incorretas;
- CBD-48/64: cobertura completa, zero incorretas.

O centro CBD-16 sozinho discordou da referência em 494 casos. O intervalo
evitou que essas aproximações fossem certificadas como decisões.

## Circuito por precisão

Com bound local e decisão `x>=0,5`:

- CBD-16: 417 constraints;
- CBD-32: 709;
- CBD-48: 1.001;
- CBD-64: 1.293.

CBD-32 comprimido em 10 folds produziu prova de 10.752 bytes.

## Limitações conhecidas

- Adult e ECEF ainda são avaliados off-circuit.
- O modelo Adult usa somente variáveis numéricas.
- A acurácia de 69,63% no subconjunto serializado é esperada porque ele
  superamostra casos próximos da fronteira; a acurácia completa é 80,87%.
- A origem/autenticidade de coordenadas ECEF está fora do escopo.

## Próximo passo

Consolidar testes e resultados; manter como trabalho futuro as step circuits
específicas de dot product e ECEF caso o baseline FP32 confirme vantagem.

## Continuidade para outra IA

Regere o modelo com o script e o arquivo oficial `adult.data`; o artefato
registra SHA-256, seed, hiperparâmetros e DOI.

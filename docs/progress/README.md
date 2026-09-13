# Diário de progresso

Este diretório é o ponto de retomada para pessoas e agentes automatizados.

## Como continuar

1. Leia [`../thesis-execution-plan.md`](../thesis-execution-plan.md).
2. Leia o último registro da tabela abaixo.
3. Confirme o estado atual executando os comandos de verificação indicados no
   registro.
4. Crie um novo arquivo numerado; não altere registros concluídos.

## Registros

| ID | Data | Estado | Avanço |
|---:|---|---|---|
| [0001](0001-inventario-inicial.md) | 2026-07-05 | concluído | Plano persistente e inventário inicial |
| [0002](0002-semantica-cbd-off-circuit.md) | 2026-07-05 | concluído | Semântica e referência racional CBD |
| [0003](0003-step-circuit-cbd.md) | 2026-07-05 | concluído | Step circuit CBD e smoke test Nova |
| [0004](0004-avaliacao-bound-global.md) | 2026-07-05 | concluído, resultado negativo | Cobertura do bound global CBD |
| [0005](0005-bound-local-cbd.md) | 2026-07-05 | concluído | Bound local, circuito e cobertura |
| [0006](0006-decisao-trivalorada-circuito.md) | 2026-07-05 | concluído | Predicado CBD constrained |
| [0007](0007-referencia-fp32-restrita.md) | 2026-07-05 | parcial, circuito pendente | Referência e validação FP32 |
| [0008](0008-workloads-adult-ecef.md) | 2026-07-05 | concluído off-circuit | Adult e proximidade ECEF |
| [0009](0009-auditoria-da-rodada.md) | 2026-07-05 | concluído com pendências explícitas | Auditoria, testes e handoff |
| [0010](0010-gadgets-fp32-exatos.md) | 2026-07-05 | concluído | Gadgets FP32 exatos em R1CS |
| [0011](0011-validacao-fp32-adversarial.md) | 2026-07-05 | concluído, Berkeley pendente | Validação diferencial e adversarial |
| [0012](0012-benchmark-go-no-go-fp32.md) | 2026-07-05 | concluído, decisão go | Benchmark homogêneo CBD-32 × FP32 |
| [0013](0013-semantica-cbd-sequencial.md) | 2026-07-05 | concluído, hipótese falsificada | Semântica sequencial e contraexemplo de largura |
| [0014](0014-circuito-cbd-sequencial.md) | 2026-07-05 | concluído | Circuito CBD sequencial e testes adversariais |
| [0015](0015-ablacao-fusao-representacao.md) | 2026-07-05 | concluído | Ablação causal CBD fusionado × sequencial × FP32 |
| [0016](0016-certified-floating-dyadic.md) | 2026-07-13 | v1 concluída, go parcial | CFD IEEE-shaped, enclosure R1CS e cobertura |
| [0017](0017-protocolo-pareto.md) | 2026-07-30 | concluído | Protocolo da fronteira, fixado antes de executar |
| [0018](0018-circuitos-parametrizados.md) | 2026-07-30 | concluído | CLI parametrizada e `profile_hash_hex` como chave de junção |
| [0019](0019-harness-fronteira.md) | 2026-07-30 | concluído | Harness de cobertura com esquema único nas quatro famílias |
| [0020](0020-fronteira-pareto.md) | 2026-07-30 | concluído, go parcial resolvido | Fronteira por regime, garantida × observada |
| [0021](0021-escalonamento-em-circuito.md) | 2026-07-30 | concluído | Vínculo de escalonamento em circuito, com custo medido |
| [0022](0022-validacao-e-consolidacao.md) | 2026-07-30 | concluído | Berkeley SoftFloat e soundness por enumeração exaustiva |
| [0023](0023-medicao-estatistica.md) | 2026-07-30 | concluído | Tempos com desenho pareado e intervalo de confiança |
| [0024](0024-cdad-v1.md) | 2026-08-30 | v1 experimental | CDAD adaptativa, score R1CS, quatro workloads e Nova curto |
| [0025](0025-semantica-numerica-funcoes.md) | 2026-09-01 | v1 off-circuit concluída | Numeric contract, exp/sigmoid, Taylor, Padé, Gauss-Legendre e decisão |
| [0026](0026-cdad-contract-audit.md) | 2026-09-08 | auditoria off-circuit | Checker composicional, quatro políticas, referência dirigida e gates competitivos |
| [0027](0027-qualification.md) | 2026-09-11 | pacote dinâmico | Matriz completa, prova vinculada pública, gráficos e campanha reproduzível |
| [0028](0028-comparacoes-literatura-e-graficos.md) | 2026-09-13 | pesquisa documentada; execução pendente | Literatura, dez figuras propostas, baselines competitivos e critérios de comparação |
| [0029](0029-grupo-a-figuras-auditadas.md) | 2026-09-13 | Grupo A concluído | Classificação corrigida, auditoria racional, figuras 1–4 PDF/SVG e reprodução das tabelas |

O capítulo consolidado está em
[`../chapter-certified-frontier.md`](../chapter-certified-frontier.md).

## Modelo obrigatório

Cada novo registro deve conter:

1. objetivo e estado;
2. arquivos modificados;
3. decisões e hipóteses;
4. comandos executados;
5. testes, métricas e evidências;
6. tentativas malsucedidas;
7. limitações conhecidas;
8. próximo passo;
9. instrução de continuidade para outra IA.

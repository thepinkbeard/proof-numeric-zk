# 0023 — Medição estatística de tempos

Data: 2026-07-30. Estado: concluído. O resultado é negativo e útil: tempo de
parede não separa as representações a 10 folds, e a razão é mensurável.

## Objetivo e estado

As 3 repetições de 0006 não sustentavam ordenação de tempos, e o próprio
`docs/report.md` reconhecia isso em duas passagens. Faltavam repetições e
intervalo de confiança.

Ao montar o experimento ficou claro que o problema não era só tamanho de amostra,
era desenho. Rodar as N repetições de uma representação e depois as N da seguinte
confunde representação com tempo de parede: se a máquina esquenta, entra em
throttling ou pega uma tarefa de fundo no meio da série, o efeito cai inteiro
sobre a representação que estava rodando naquele momento, e vira diferença
aparente entre representações. Com 3 repetições em bloco, uma única interrupção
do sistema decide o resultado.

## Arquivos modificados

- `scripts/run_timing_benchmarks.sh` (novo): 30 rodadas intercaladas.
- `scripts/analyze_timing.py` (novo): bootstrap e comparação pareada.
- `results/timing/` (novo): 150 execuções.
- `results/timing_summary.json` (novo).

## Decisões e hipóteses

**Intercalar em vez de blocar.** Cada rodada executa todas as representações, em
ordem embaralhada dentro da rodada. Deriva lenta afeta todas quase igualmente, e
o que sobra é comparável dentro da rodada. O embaralhamento evita que uma
representação fique sempre em primeiro lugar e herde sistematicamente o estado de
cache deixado pela última da rodada anterior.

**Comparação pareada.** É o que o desenho intercalado compra. Se a máquina
esquentou na rodada 17, todas as representações daquela rodada ficaram lentas
juntas, e a diferença entre elas dentro da rodada não sofre. Comparar diferenças
pareadas remove a variância entre rodadas, que é justamente a que dominava.

**Bootstrap e não teste t.** Os tempos são assimétricos à direita, com cauda de
interrupções do sistema. A hipótese de normalidade do t não se sustenta; o
percentil bootstrap não precisa dela.

**Rodada de aquecimento descartada.** A primeira execução paga cache frio de
página e de instruções, e entraria na amostra como outlier sistemático.

**Quando o intervalo cruza zero, o resultado é "não distingue".** Não é "são
iguais" nem "a média sugere que". O script reporta `separated` como booleano
explícito para que essa distinção não se perca na leitura.

## Comandos executados

```
ROUNDS=30 STEPS=10 scripts/run_timing_benchmarks.sh
python3 scripts/analyze_timing.py
```

## Testes, métricas e evidências

30 rodadas × 5 representações = 150 execuções, 10 folds, `nearest-even`, com
compressão. Coeficiente de variação entre 3,8% e 23,6%.

| representação | prova por passo | IC95 | compressão | IC95 |
| --- | --- | --- | --- | --- |
| standard | 0,0433 s | [0,0427, 0,0440] | 1,607 s | [1,558, 1,667] |
| cbd-sequential | 0,0439 s | [0,0433, 0,0447] | 1,604 s | [1,560, 1,659] |
| cfd | 0,0441 s | [0,0436, 0,0447] | 1,610 s | [1,573, 1,647] |
| fp32 | 0,0446 s | [0,0440, 0,0452] | 1,614 s | [1,585, 1,645] |
| cbd | 0,0449 s | [0,0435, 0,0474] | 1,609 s | [1,571, 1,660] |

**12 de 60 comparações pareadas separam com IC95.** E onde separam, a magnitude
desmonta a leitura ingênua: FP32 é mais lento que ponto fixo por 1,3 ms por
passo, uma diferença de 3%, enquanto o circuito de etapa do FP32 tem **5,9 vezes**
mais constraints. Compressão não separa em nenhum par. CBD contra FP32 não separa
em nenhuma métrica de prova.

**A explicação está no overhead estrutural do Nova:**

| representação | etapa | overhead Nova | total | etapa / total |
| --- | --- | --- | --- | --- |
| standard | 292 | 11.376 | 11.668 | 2,5% |
| cbd | 709 | 11.377 | 12.086 | 5,9% |
| cbd-sequential | 782 | 11.377 | 12.159 | 6,4% |
| cfd | 880 | 11.378 | 12.258 | 7,2% |
| fp32 | 1.721 | 11.376 | 13.097 | 13,1% |

O circuito de etapa é de 2,5% a 13,1% do circuito aumentado. Uma diferença de
5,9× na etapa vira 1,12× no total, e 1,12× no total desaparece sob 4% de ruído de
máquina. Não é que os tempos sejam iguais — é que a 10 folds esta configuração
não tem resolução para distingui-los.

**A consequência metodológica**, que é o resultado desta rodada: *raw step
constraints* é a métrica de custo correta para a fronteira, e não por
conveniência. Tempo a 10 folds mede predominantemente o overhead do Nova, que é
idêntico entre representações por construção; constraints medem o que de fato
varia. A escolha feita no [protocolo](../pareto-protocol.md) fica justificada por
medição, não por suposição.

Isto também fecha, com número, a ressalva do `report.md` de que "três repetições
não permitem ordenar prover ou verificador com confiança". Trinta repetições com
desenho pareado também não permitem, e agora sabe-se por quê.

## Tentativas malsucedidas

A primeira execução completa saiu com CV entre 30% e 90% e 16 de 60 pares
"separando" — mais separações que a execução limpa, o que já é sinal de que
estavam medindo ruído. A causa: a suíte de testes do workspace e o clippy rodaram
em paralelo com o benchmark. Os dados foram descartados e a execução refeita com
a máquina sem outra carga.

Vale registrar a leitura errada que quase ficou no lugar: `uptime` acusa load
average de 10 a 14 durante o benchmark, o que parece contaminação, mas é o
próprio `nova-circuit`, que roda a ~957% de CPU. Load alto aqui é o benchmark
trabalhando, não interferência. O que distingue os dois casos é o CV, não o load.

## Limitações conhecidas

1. Uma única máquina, Apple Silicon, sob outras cargas de usuário possíveis. Os
   tempos não são portáveis; os constraints são.
2. 10 folds. A 1.000 folds o overhead estrutural amortiza e a diferença de etapa
   pode emergir. Não foi medido — cada rodada de 1.000 folds custa ~100× mais, e
   o desenho pareado exigiria repetições proporcionais.
3. `cfd` roda em K=1 e as demais em K=2, porque o circuito CFD exige K=1. As
   comparações que envolvem `cfd` carregam essa diferença de configuração.
4. O bootstrap é sobre a média. Para caudas pesadas a mediana seria mais estável,
   e está no JSON, mas as comparações pareadas usam a média.

## Próximo passo

Nada bloqueia. O capítulo está em `docs/chapter-certified-frontier.md`.

## Instrução de continuidade para outra IA

Rode `ROUNDS=30 STEPS=10 scripts/run_timing_benchmarks.sh` **sem nada mais na
máquina** — inclusive sem `cargo test` em outro terminal, que foi o que
invalidou a primeira execução. O sinal de contaminação é CV acima de ~25% em
`analyze_timing.py`, não o load average.

Se for medir a 1.000 folds para ver se a diferença de etapa emerge, ajuste
`STEPS` e espere horas; mantenha o desenho intercalado, que é o que torna a
comparação possível.

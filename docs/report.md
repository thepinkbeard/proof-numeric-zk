# Relatório experimental

Data: 2026-07-13.

## Escopo medido

O experimento compara FP32, FP64, Q16.16, Q32.32, limbs `16×1` a `16×4`
com truncagem e round-to-nearest-ties-to-even, e uma variante compensada em
grade `S²`. Os horizontes são 10, 30, 100, 1.000 e 10.000, para
`r={3.5,3.7,3.9,4.0}` e `x0=1/10`.

A referência é racional exata nos testes curtos e MPFR nos horizontes longos.
Para `N` passos, a precisão automática é `max(256, 2N+128)` bits. Cada linha
registra também um bound conservador de erro da referência.

## Resultados numéricos

No regime periódico `r=3.5`, FP64, limbs `16×3`, limbs `16×4` e a variante
compensada `16×2` não excederam erro absoluto `10^-6` em 10.000 passos. Isso
não se transfere aos regimes caóticos.

Primeira iteração com erro absoluto maior que `10^-6`:

| Formato | r=3.7 | r=3.9 | r=4.0 |
|---|---:|---:|---:|
| FP32 | 9 | 9 | 5 |
| FP64 | 77 | 54 | 36 |
| Q16.16 truncado | 0 | 0 | 0 |
| Q32.32 truncado | 29 | 15 | 15 |
| limbs 16×2 nearest | 30 | 16 | 18 |
| limbs 16×3 nearest | 65 | 45 | 32 |
| limbs 16×4 nearest | 94 | 65 | 45 |
| compensado 16×2 | 99 | 68 | 45 |

O erro inicial de Q16.16 já excede `10^-6`; por isso aparece como iteração
zero. Em `r=3.7`, o primeiro flip entre os três predicados ocorre nos passos
46 (FP32), 101 (FP64), 14 (Q16.16), 63 (Q32.32 truncado), 114 (limbs 16×4)
e 135 (compensado 16×2).

A compensação `16×2` posterga a divergência em relação a `16×2`, mas armazena
efetivamente uma grade de 64 bits. Ela também quantiza a expressão logística
completa uma vez, enquanto a variante comum quantiza após `x(1-x)` e novamente
após multiplicar por `r`. Logo, o resultado sustenta que a grade fina ajuda,
mas ainda não isola causalmente “carregar o resto” de “usar mais bits e outra
ordem de quantização”.

Dados completos: `results/numeric_summary_n10000.csv`.

![Primeira divergência](../results/first_divergence_r3_7.svg)

## Resultados do circuito

### Primeiro marco CBD

A primeira step circuit CBD fusionada foi medida em `K=2`, nearest-even e
bound de Lipschitz para o raio:

| Configuração | Step constraints | Range/modeladas | Estado | Circuito Nova aumentado |
|---|---:|---:|---:|---:|
| CBD `16×2` | 466 | 448 | 5 FE | 11.842 |

Um smoke test de duas etapas concluiu setup, folding e verificação recursiva.
O custo bruto já mostra um resultado importante: esta implementação CBD é mais
cara que `standard 16×2 nearest-even` (221), mas mais barata que o modelo
FP32 bit-level local (3.910). Ela também é mais cara que o baseline FP32
otimista estimado (342), portanto ainda não sustenta a hipótese de dominância.
O baseline FP32 homogêneo continua necessário para o marco go/no-go.

A avaliação off-circuit em 1.000 passos identificou uma limitação mais
importante: o bound global `R'=ceil(rR)+1` satura rapidamente. Em `r=3.5`, o
perfil CBD-32 produziu somente 17 decisões definitivas para os limiares 0,25 e
0,50; 984 de 1.001 estados ficaram indeterminados. Nos regimes caóticos a
cobertura também ficou abaixo de 4% mesmo com 64 bits. Não houve decisão
certificada incorreta, mas a disponibilidade é insuficiente. Essa variante
fica preservada como baseline sound e negativo; a próxima implementação deve
usar o bound local da derivada e o termo quadrático do erro.

O bound local foi então implementado. Em `r=3.5`, CBD-32/48/64 certificaram
100% das 1.001 decisões no limiar 0,50, sem falsos certificados. Em `r=3.7`,
CBD-64 certificou 122 decisões antes de se tornar conservador; em `r=3.9`,
85; em `r=4.0`, 60. A step circuit local `K=2` custa 557 constraints brutas e
11.933 no circuito Nova aumentado. O acréscimo de 91 constraints sobre o
bound global recupera a cobertura completa no regime estável e deixa o custo
próximo da variante compensada nearest-even (567).

Ao constrained também o predicado `x>=0,5`, o custo local subiu para 709
constraints brutas, 12.086 constraints Nova aumentadas e 6 elementos de
estado. Esse número é o custo relevante para uma decisão certificada; os 557
anteriores cobriam apenas a transição numérica. A witness de decisão adulterada
é rejeitada.

A step circuit medida verifica truncagem e também possui modo
`nearest-ties-to-even` constrained. A tabela principal abaixo é da variante
truncada regenerada após adicionar resto pequeno canônico e bits explícitos de
seleção de arredondamento:

- limbs de 16 bits;
- bit e canonicalidade do valor um;
- reconstrução dos numeradores;
- complemento `S-N`;
- produto `N(S-N)`;
- relação `A=S*Q+d`;
- bit `rounded_up` e `Q=floor+rounded_up`;
- relação `R*Q=D*floor'+e`;
- one-hot para `0 <= e < D`;
- bit `rounded_up` final e `N'=floor'+rounded_up`;
- ranges de quocientes e restos;
- incremento do contador.

| Configuração | Bits | Step constraints | Range checks | Estado | Circuito Nova aumentado |
|---|---:|---:|---:|---:|---:|
| 16×1 / Q16.16 | 16 | 130 | 92 | 3 FE | 9.950 |
| 16×2 / Q32.32 | 32 | 214 | 174 | 4 FE | 11.589 |
| 16×3 | 48 | 298 | 256 | 5 FE | 11.674 |
| 16×4 | 64 | 382 | 338 | 6 FE | 11.759 |

Os totais aumentam em 84 constraints por limb nesta implementação. Range
checks e reconstruções dominam o circuito específico. O salto não linear entre
`k=1` e `k=2` no circuito aumentado vem do padding/estrutura interna de Nova;
depois disso, o overhead fica praticamente constante.

Smoke benchmarks comprimidos em `K=2`, `N=10`:

| Variante | Step constraints | Range checks | Estado | Circuito Nova aumentado | Prova |
|---|---:|---:|---:|---:|---:|
| standard / truncate | 214 | 174 | 4 FE | 11.589 | 10.688 B |
| standard / nearest-even | 221 | 174 | 4 FE | 11.596 | 10.688 B |
| carry / truncate | 391 | 375 | 4 FE | 11.766 | 10.688 B |
| compensated / fine-grid truncate | 420 | 410 | 5 FE | 11.796 | 10.720 B |
| compensated / fine-grid nearest-even | 567 | 557 | 5 FE | 11.943 | 10.720 B |

A variante carry substitui a multiplicação agregada por `K²` produtos de
limbs, dígitos e carries explícitos. Para `K=2`, ela custa 391 constraints
brutas contra 214 da variante agregada truncada. A variante compensada truncada
custa 420 constraints brutas, principalmente por carregar `X` em grade `S²`,
complemento fino, quociente fino, resto fino e residual de saída. O modo
`nearest-even` compensado acrescenta 147 constraints nessa versão, vindas da
comparação de divisor constante, empates e seleção do bit de arredondamento. No
circuito Nova aumentado o acréscimo parece menor porque o overhead estrutural
domina esta instância pequena.

Tabela carry truncada por `K`:

| k | Bits | Step constraints | Mult. limb | Range/carry bucket | Circuito Nova aumentado |
|---:|---:|---:|---:|---:|---:|
| 1 | 16 | 208 | 1 | 198 | 10.028 |
| 2 | 32 | 391 | 4 | 375 | 11.766 |
| 3 | 48 | 576 | 9 | 552 | 11.952 |
| 4 | 64 | 763 | 16 | 729 | 12.140 |

![Constraints por k](../results/constraints_by_k.svg)

## Baseline FP32 homogêneo

O baseline FP32 positivo-normal agora possui gadget R1CS bit-exato no mesmo
backend Nova. A representação circuital separa expoente enviesado,
significando explícito de 24 bits e bit de zero. Multiplicação e `1-x`
verificam normalização, divisão euclidiana e
round-to-nearest-ties-to-even. A etapa preserva a ordem FP32:
`1-x`, `x(1-x)` e multiplicação por `r`, sem FMA.

| Circuito FP32 | Constraints brutas | Range checks |
|---|---:|---:|
| multiplicação isolada | 474 | 417 |
| `1-x` isolado | 924 | 855 |
| etapa logística com decisão | 1.721 | 1.551 |

Dez mil vetores positivos-normais coincidiram bit a bit com a referência
software independente; 256 deles também foram sintetizados em R1CS. Os testes
cobrem empate par/ímpar, carry de arredondamento, alinhamento variável de
expoente, zero, extremos de `[0,1]`, witness adulterado e forma circuital
independente do witness.

Comparação homogênea de 10 folds, com três repetições para os tempos:

| Métrica | CBD-32 local | FP32 restrito exato |
|---|---:|---:|
| Step constraints | 709 | 1.721 |
| Range checks | 689 | 1.551 |
| Estado | 6 FE | 5 FE |
| Circuito Nova aumentado | 12.086 | 13.097 |
| Prover total médio | 0,315 s | 0,322 s |
| Compressão média | 0,965 s | 1,052 s |
| Verificação comprimida média | 22,16 ms | 22,13 ms |
| Prova | 10.752 B | 10.720 B |
| Pico RSS médio | 158,1 MB | 161,5 MB |

O FP32 custa 2,43 vezes mais constraints brutas; 90,1% de sua etapa são range
checks. No circuito Nova aumentado a diferença cai para 8,4% devido ao
overhead estrutural. O resultado supera o limite do modelo otimista anterior
de 342 constraints e fica abaixo da estimativa bit-level de 3.910.

O go/no-go é positivo para continuar investigando CBD, mas não demonstra uma
vitória universal: CBD usa uma quantização fusionada e oferece decisão sobre um
intervalo real, enquanto FP32 reproduz três operações separadas sobre seu
próprio valor finito. O baseline também exclui negativos, subnormais, NaN,
infinito e caminhos excepcionais de overflow/underflow.

A validação Berkeley SoftFloat/TestFloat continua bloqueada neste host
`aarch64-apple-darwin`: `softfloat-sys 0.1.4` não possui regras de build para o
alvo cruzado x86_64 e o daemon Docker local não está disponível. O teste para
host x86_64 real permanece em `tools/softfloat-validation`.

## Ablação de fusão

Foi implementado um CBD sequencial com a mesma ordem operacional do baseline
FP32: complemento, produto quantizado e multiplicação por `r` quantizada. O
bound mantém a dependência entre `x` e `1-x`; não usa multiplicação intervalar
independente.

| Perfil | CBD fusionado | CBD sequencial | FP32 sequencial |
|---|---:|---:|---:|
| 16 bits | 417 | 472 | 1.721 |
| 32 bits | 709 | 782 | 1.721 |
| 48 bits | 1.001 | 1.092 | 1.721 |
| 64 bits | 1.293 | 1.402 | 1.721 |

Todos os perfis CBD sequenciais permaneceram abaixo do FP32 restrito. Em
CBD-32, retirar a fusão acrescentou 73 constraints, ou 10,3%, mas o FP32
continuou 2,20 vezes mais caro em constraints brutas.

Cobertura no limiar `0,50`, em 1.001 estados:

| r | CBD-32 fusionado | CBD-32 sequencial | Incorretas |
|---:|---:|---:|---:|
| 3,5 | 1.001 | 1.001 | 0 |
| 3,7 | 63 | 58 | 0 |
| 3,9 | 34 | 31 | 0 |
| 4,0 | 28 | 27 | 0 |

Nos três limiares combinados, o CBD-32 sequencial certificou 3.359 decisões
contra 3.377 do fusionado, uma redução de 0,53%. O fusionado domina o
sequencial na fronteira CBD medida — menor custo e cobertura igual ou maior —
mas a variante sequencial cumpre seu papel causal: a vantagem sobre FP32 não
é explicada apenas pela fusão.

Benchmark de 10 folds em `K=2`, com três repetições:

| Métrica | CBD fusionado | CBD sequencial | FP32 |
|---|---:|---:|---:|
| Step constraints | 709 | 782 | 1.721 |
| Nova aumentado | 12.086 | 12.159 | 13.097 |
| Prover total médio | 0,315 s | 0,324 s | 0,322 s |
| Compressão média | 0,965 s | 1,018 s | 1,052 s |
| Verificação comprimida | 22,16 ms | 22,28 ms | 22,13 ms |
| Prova | 10.752 B | 10.752 B | 10.720 B |

O overhead Nova reduz a diferença observável de tempo; três repetições não
permitem ordenar prover ou verificador com confiança. A conclusão robusta é a
contagem determinística de constraints. Os dados completos estão em
`results/cbd_fusion_ablation.json`.

> Atualização de [0023](progress/0023-medicao-estatistica.md): trinta rodadas
> intercaladas com comparação pareada e intervalo bootstrap **também** não
> permitem ordenar, e agora sabe-se por quê. O circuito de etapa é de 2,5% a
> 13,1% do circuito aumentado do Nova; uma diferença de 5,9× na etapa vira 1,12×
> no total, abaixo do ruído de máquina. A ressalva acima deixou de ser uma
> limitação de amostra e passou a ser um resultado medido.

Uma hipótese do plano foi falsificada: a largura dos intervalos fusionado e
sequencial não é totalmente ordenada ponto a ponto. Centros arredondados
diferentes e o recorte em `[0,1]` produzem contraexemplos. Ambos permanecem
sound, e nenhuma decisão certificada incorreta foi observada.

## Workloads complementares CBD

O modelo logístico determinístico com seis atributos numéricos do UCI Adult
atingiu 80,87% no conjunto de teste completo. O artefato de avaliação retém
1.857 casos, enfatizando margens próximas de zero.

| Workload/perfil | Casos | Definitivas | Indeterminadas | Incorretas |
|---|---:|---:|---:|---:|
| Adult CBD-16 | 1.857 | 1.856 | 1 | 0 |
| Adult CBD-32 | 1.857 | 1.857 | 0 | 0 |
| ECEF CBD-16 | 2.000 | 837 | 1.163 | 0 |
| ECEF CBD-32 | 2.000 | 1.991 | 9 | 0 |
| ECEF CBD-48 | 2.000 | 2.000 | 0 | 0 |

O workload ECEF usa distância de corda quadrática entre vetores unitários e
limiar equivalente a 100 km. Metade dos casos foi gerada muito próxima do
limiar. Em CBD-16, o valor central sozinho errou 494 decisões; a semântica
certificada converteu esses casos em `Indeterminate`, sem autorizar uma
decisão errada.

## Escala e compressão CBD

| Perfil | Step constraints com decisão | Estado | Nova aumentado |
|---|---:|---:|---:|
| CBD-16 | 417 | 5 FE | 11.793 |
| CBD-32 | 709 | 6 FE | 12.086 |
| CBD-48 | 1.001 | 7 FE | 12.379 |
| CBD-64 | 1.293 | 8 FE | 12.672 |

Em CBD-32, 10 folds comprimidos geraram prova de 10.752 bytes, compressão em
0,956 s e verificação comprimida em 21,81 ms.

## IVC e compressão

Perfil: `nova-snark 0.73.0`, Pallas/Vesta, Pedersen+IPA, limbs `16×2`,
truncagem. Três repetições após aquecimento:

| Passos | Prover total médio | Tempo/fold | Compressão | Verificação comprimida | Prova | Pico RSS |
|---:|---:|---:|---:|---:|---:|---:|
| 10 | 0,310 s | 31,05 ms | 1,040 s | 21,66 ms | 10.688 B | 159,1 MB |
| 100 | 3,043 s | 30,43 ms | 0,992 s | 21,88 ms | 10.688 B | 158,8 MB |
| 1.000 | 30,279 s | 30,28 ms | 1,091 s | 22,52 ms | 10.688 B | 160,3 MB |

Os números demonstram estado e prova de tamanho constante e custo incremental
aproximadamente constante por fold. O último desvio da verificação em 1.000
passos é pequeno, mas três repetições ainda são uma amostra limitada. Para os
tempos com intervalo de confiança, ver `results/timing_summary.json` e
[0023](progress/0023-medicao-estatistica.md).

Máquina: Apple M5 Pro, 15 cores, 24 GB, macOS 27.0. Toolchain fixado:
Rust 1.85.0. O pico de memória é `ru_maxrss` via `getrusage(RUSAGE_SELF)`,
registrado em bytes no JSON.

## Certified Floating Dyadic

A CFD v1 representa um centro normalizado por significando e expoente,
acrescenta raio inteiro em ULPs, `+0` canônico e fallback absorvente `wide`.
O centro honesto usa RNE; o circuito prova inclusão da imagem intervalar e a
segurança da decisão, não equivalência bit a bit da operação. Corredor de
expoentes, domínio, limiares e fallback integram o hash SHA-256 do perfil.

| Workload/topologia | CFD-24 | FP32 exato | Redução |
|---|---:|---:|---:|
| logística sequencial | 1.003 | 1.721 | 41,7% |
| score afim sequencial | 846 | 1.418 | 40,3% |
| logística fusionada/checkpoint | 777 | — | ablação |

O estado CFD usa 7 FE. Em 43 configurações off-circuit, abrangendo p16, p24,
p32, corredores de 5, 9 e 17 lanes e as duas topologias, não houve decisão
definitiva incorreta nem violação da referência racional/MPFR. O score afim e
o regime periódico `r=3.5` mantiveram cobertura integral em 1.001 estados para
o limiar principal.

O resultado em caos é deliberadamente reportado como negativo. Para `r=3.7`,
CFD-24 torna-se `wide` entre os passos 33 e 41; para `r=4.0`, entre 13 e 14,
conforme topologia. Ampliar o corredor de 9 para 17 lanes não melhora de forma
material esse horizonte, pois a expansão da incerteza, e não a falta de
expoente, passa a dominar.

Assim, a CFD supera o alvo de 30% contra FP32 nos dois workloads e satisfaz o
critério de soundness medido, mas ainda não demonstra não-dominância contra a
fronteira CBD/fixed-point completa. O veredito científico atual é **go
parcial**: existe contribuição de representação e aritmetização, mas a
alegação de fronteira Pareto exige novos perfis de raio e comparação de custo
por decisão definitiva no workload afim.

## Hipóteses

- **H1: sustentada com ressalva.** `16×2` oferece 32 bits fracionários em
  `[0,1]`, mais que os 24 bits de significando de FP32, mas não reproduz a
  semântica IEEE 754.
- **H2: sustentada no subconjunto medido, inclusive sem fusão.** CBD-32
  sequencial custa 782 constraints contra 1.721 do FP32 positivo-normal exato
  no mesmo backend e ordem operacional. Permanecem diferentes as semânticas
  numéricas e o escopo de classes representáveis.
- **H3: parcialmente sustentada.** A grade compensada posterga divergência,
  porém adiciona estado e muda o ponto de quantização.
- **H4: sustentada empiricamente.** Em `r=3.7,3.9,4.0`, todos os formatos
  finitos divergem; mais precisão geralmente posterga a divergência.
- **H5: não há ótimo universal.** A eficiência bruta
  `bits/step-constraints` sobe de 0,123 (`k=1`) para 0,168 (`k=4`), mas o ganho
  de trajetória depende de `r`, horizonte e limiar.
- **H6: sustentada no domínio restrito.** O baseline homogêneo quantifica o
  custo de expoente, normalização e RNE: 1.551 das 1.721 constraints FP32 são
  range checks. A afirmação não se estende ainda ao IEEE 754 geral.

## Respostas diretas

- **Supera Q16.16?** Sim em resolução e, nos casos caóticos medidos, em tempo
  até divergência; custa mais estado e range checks.
- **Supera Q32.32?** `16×2` não: na configuração equivalente, ambos têm a
  mesma grade e operações. `16×3` e `16×4` superam em precisão, com maior custo.
- **Supera FP32?** Em precisão fracionária, `16×2` sim. Em toda trajetória,
  depende do arredondamento e do parâmetro; os dados caóticos favorecem os
  formatos de 48/64 bits, não garantem vitória universal de `16×2`.
- **É mais barata que IEEE 754 em circuito?** Neste domínio restrito e backend,
  sim: CBD-32 sequencial usa 782 constraints contra 1.721 do FP32 exato com a
  mesma ordem operacional. Isso ainda não é uma alegação sobre IEEE 754 geral.
- **O residual ajuda?** Ajuda empiricamente, mas não gratuitamente. Agora há
  uma primeira aritmetização `K=2`: ela custa 420 constraints brutas em
  truncagem e 567 com `nearest-even`, contra 214 da truncada padrão, e ainda
  não isola totalmente precisão adicional/ordem operacional.
- **Melhor configuração atual?** `16×3 nearest` é o compromisso prudente;
  `16×4` maximiza fidelidade, enquanto `16×2` minimiza custo entre as opções
  com precisão superior a FP32.

## Limitações e ameaças à validade

1. A step circuit atual usa relações agregadas e decomposição em bits; a
   variante carry mede uma convolução limb-a-limb para `K=1..4`, incluindo
   endpoints, mas nesta rodada só foi benchmarkada com truncagem.
2. O circuito residual compensado existe apenas para `K=2`, com truncagem e
   `nearest-even` na grade fina. Ele ainda precisa de extensão para outros `K`
   e carries limb-a-limb.
3. Q32.32 e limbs `16×2` são deliberadamente equivalentes nesta ordem de
   operações; chamá-los de formatos numericamente distintos seria enganoso.
4. A referência MPFR possui bound conservador, não é uma trajetória racional
   exata em 10.000 passos.
5. O baseline FP32 homogêneo cobre apenas positivos normais e `+0`, não IEEE
   754 geral, e não há perfil BN254/Grumpkin. A memória reportada é RSS máximo
   do processo, não uma decomposição por fase interna.
6. Os benchmarks são específicos desta máquina, toolchain e perfil IPA.
7. A validação com Berkeley TestFloat/SoftFloat, enumeração exaustiva de
   formatos reduzidos e o vínculo circuital de `next_profile` permanecem
   pendentes e não são alegados como concluídos.

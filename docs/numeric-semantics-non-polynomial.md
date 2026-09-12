# Semântica numérica e aproximações não polinomiais

## Pergunta experimental

Uma representação, isoladamente, não determina a semântica certificada. O
objeto relevante é o contrato que une representação, operações, aproximação,
domínio e erro, e se esse contrato basta para preservar a decisão da aplicação.

Os experimentos existentes de mapa logístico, affine, Adult e ECEF continuam
como grupo composicional/iterativo. Esta extensão acrescenta o grupo de
fidelidade de primitivas, inicialmente `exp` e `sigmoid`.

## Perfil público

`NumericContractProfile` é independente do `NumericProfile` legado da CBD e do
`ExperimentProfile` usado na fronteira Nova. Essa separação preserva os hashes
v1/v2 existentes. O novo hash SHA-256 cobre JSON canônico de schema v1, com
mapas ordenados e racionais em forma textual reduzida.

O perfil declara representação, largura, escala, arredondamento, rescaling,
overflow, underflow, valores excepcionais, domínio, método, ordem, forma de
avaliação, fonte de coeficientes/nós e quatro componentes do contrato de erro.

Proveniência e força de evidência são eixos diferentes:

| Proveniência | Significado |
|---|---|
| `PaperDerived` | fórmula ou análise explicitamente presente no paper |
| `ArtifactDerived` | comportamento observado no artefato oficial |
| `RepositoryDerived` | baseline ou mecanismo já especificado no projeto |
| `OurExtension` | nova composição proposta neste trabalho |

| Evidência | Pode certificar? |
|---|---:|
| `Formal` | sim |
| `Analytical` | sim |
| `ExhaustiveFiniteDomain` | sim |
| `ObservedGrid` | não |
| `ModelAssumption` | não |

`BoundedApproximation` só é derivada quando todos os componentes relevantes e
a composição são certificantes. Não é uma propriedade hardcoded de fixed-point.

## Paper e artefato de Sridhar

Fonte primária: S. Sridhar et al., *Efficiently Provable Approximations for
Non-Polynomial Functions*, USENIX Security 2026. Artefato oficial:
`10.5281/zenodo.21706296`.

Conceitos usados diretamente:

- fixed-point quantizado e erro honesto inevitável;
- slack que a relação verificável permite a um witness;
- Taylor e Padé como aproximações polinomiais/racionais;
- Gauss-Legendre aplicado à derivada da inversa;
- relações estáveis que evitam denominadores próximos de zero;
- pesos GL somando dois e crescimento em termos sem crescimento equivalente da
  profundidade da relação;
- quatro bits de perda como bound agregado em relation space na análise GL:
  dois no produto, três após o inverse check e um adicional na soma ponderada.
  O runner histórico materializou isso como `16 * 2^-fractional_bits`.
  **Correção da auditoria v2:** essa tradução não estabelece um bound universal
  para a relação implementada. O valor é preservado como `ModelAssumption` de
  nossa extensão, não como garantia diretamente demonstrada pelo paper.

O texto define a codificação por `floor(a*2^s)`. O artefato usa
`mpmath.nint` 1.3.0, isto é, nearest-ties-to-even. Por isso existem perfis
separados; nenhum deles é silenciosamente chamado de "a semântica Sridhar".

O artefato usa Noir/Barretenberg UltraPLONK. Este repositório usa Nova/R1CS. Os
tempos e constraints publicados pelo paper não são importados como benchmarks
homogêneos.

## Aproximações implementadas

Taylor e Padé usam coeficientes racionais exatos. Taylor é avaliado por Horner,
com arredondamento após cada multiplicação. Padé usa `[m/m]`, avalia numerador e
denominador separadamente e faz uma divisão fixed-point final.

Gauss-Legendre gera raízes e pesos deterministicamente em MPFR-512. Para
`y=exp(x)` e `y=sigmoid(x)`, as relações estáveis são:

```text
exp:     sum_i w_i (y-1)/(y r_i-r_i+y+1) = x
sigmoid: sum_i w_i (2y-1)/(2y r_i-r_i+1) = x
```

A saída honesta é a quantização canônica da solução implícita. Não se escolhe
um vizinho de grade para reduzir o residual, pois isso modelaria comportamento
adversarial. O residual, o slack da relação e o bound de saída por monotonicidade
são campos separados. O último era relativo à solução da quadratura e não
incluía erro contra a transcendental. No esquema 2 ele é ausente, até existir
justificativa completa para essa implementação. Uma relação nova com checker
próprio é descrita em [contrato composicional](cdad-compositional-contract.md).

Vetores dourados verificam os parâmetros Q16 sigmoid do artefato:

```text
GL n=4 roots:   [-56435, -22281, 22281, 56435]
GL n=4 weights: [22797, 42739, 42739, 22797]
Padé [3/3] P:   [32768, 16384, 3277, 273]
Padé [3/3] Q:   [65536, 0, 6554, 0]
```

## Gaps e decisão

Cada linha preserva:

```text
f(x)                     referência MPFR dirigida
Q(f(Q(x)))               representação isolada
A(Q(x))                  aproximação ideal
AQ(Q(x))                 execução fixed-point honesta
Accepted(..., slack)      região da relação verificável
```

Isso produz categorias `G_representation`, `G_approximation` e
`G_verification`, sem assumir que são aditivas.

Para um threshold `tau`, a decisão é certificada apenas se o enclosure total
estiver todo abaixo ou todo acima de `tau`. Sem bound total, a mesma comparação
recebe somente `ObservedSame` ou `ObservedFlip`. A API também reserva a condição
top-1/top-2 com bounds separados para futura avaliação zkML; não há workload
zkML nesta entrega.

## Matrizes e limites

`experiments/non_polynomial_v1.json` contém o sweep científico em `[-1,1]`:
Q8.8 a Q120.120, floor/RNE, Taylor `{3,5,7,9,13}`, Padé
`{2,3,4,5,8}` e GL `{2,4,8,16,32}`. A amostragem combina malha, seed fixa e
vizinhanças das fronteiras.

`experiments/sridhar_artifact_compat_v1.json` é uma faixa separada de sigmoid
em `[0,1]`, com os pares de precisão/ordem do `run_all.py`. Ela não é misturada
à fronteira principal.

Limitações deliberadas:

- não há circuito novo, constraints estimadas ou constraints medidas;
- Padé, sigmoid Taylor e o erro de quadratura contra a função permanecem
  observados quando falta bound global;
- tanh, potência fracionária e cos estão tipados, mas não entram na matriz v1;
- precisão/ordem adaptativa permanece fase futura sobre CDAD;
- o pico RSS não é atribuído por configuração; memória conceitual de witness é
  reportada separadamente.

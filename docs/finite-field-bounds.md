# Bounds no corpo finito

A primeira step circuit usa `P=16`, `1 <= K <= 4`, `S=2^(16K)` e suporta
truncagem ou `nearest-ties-to-even`. Para `K=4`:

| Intermediário | Bound inteiro |
|---|---:|
| Numerador `N` | `0 <= N <= 2^64` |
| Complemento `S-N` | `0 <= S-N <= 2^64` |
| Produto bruto `N(S-N)` | `< 2^128` |
| Quociente do produto antes/depois de arredondar | `<= 2^64` |
| Resto do produto | `< 2^64` |
| `R*Q`, com `R<=40` | `< 2^70` |
| Numerador final antes/depois de arredondar | `<= 2^64` |
| Resto por `D`, com `D<=10` | `< 10` |

Os módulos escalares dos ciclos usados por Nova têm aproximadamente 254–255
bits. Assim, todas as expressões acima são estritamente menores que o módulo e
as igualdades R1CS representam igualdades inteiras, desde que os range checks
sejam satisfeitos.

Os limbs, quocientes e restos são decompostos em bits e reconstruídos dentro do
circuito. O resto do divisor pequeno `D` também é restringido por one-hot
em `{0,...,D-1}`, evitando a aceitação de uma divisão não canônica como
igualdade modular. `is_one` é booleano e implica limbs nulos, evitando
representações múltiplas do valor um.

## Variante carry `K=1..4`

A variante `carry` mede uma aritmetização mais próxima da convolução limb-a-limb
para `N(S-N)`. Para `B=2^16`, os limbs são tratados em ordem little-endian
durante a convolução. No caso `K=2`, `N=a_1B+a_0` e `S-N=b_1B+b_0`, ela
verifica:

```text
p00 = a0*b0
p01 = a0*b1
p10 = a1*b0
p11 = a1*b1
p00 = d0 + B*c1
p01 + p10 + c1 = d1 + B*c2
p11 + c2 = d2 + B*c3
d3 = c3
floor = d3*B + d2
remainder = d1*B + d0
```

Esta versão usa quatro multiplicações não lineares de limbs e carries
range-checked para `K=2`; em geral usa `K²` multiplicações de limbs e
`2K-1` carries. O complemento também possui bit canônico `is_one`, então os
endpoints `x=0` e `x=1` são aceitos sem ambiguidade.

## Variante compensada inicial

A step circuit compensada implementada nesta rodada é restrita a `K=2`. Ela usa
`S=2^32`, `T=S²=2^64` e residual assinado codificado como `E=offset-S/2`, com
`0 <= offset <= S`. O estado representa:

```text
x = (S*V + E) / S².
```

A transição na grade fina verifica:

```text
X = S*V + offset - S/2
r_num * X * (S² - X) = r_den * S² * Y + d
Y = S*V' + offset' - S/2
```

No modo `nearest-ties-to-even`, o circuito substitui o quociente truncado por
`Y=floor+rounded_up` e prova a decisão de arredondamento comparando
`2*d` com o divisor constante `r_den*S²`. O empate usa a paridade do `floor`;
em truncagem, `rounded_up` é fixado a zero.

Bounds para `K=2`:

| Intermediário | Bound inteiro |
|---|---:|
| `X` | `0 <= X <= 2^64` |
| `X(S²-X)` | `< 2^128` |
| `r_num*X(S²-X)`, com `r_num<=40` | `< 2^134` |
| divisor `r_den*S²`, com `r_den<=10` | `< 2^68` |
| `Y` | `0 <= Y <= 2^64` |
| `d` | `< 2^68` |

Esses bounds ainda ficam abaixo do módulo escalar de Pallas/Vesta. Para `K=3`
ou `K=4`, a mesma relação fina deixaria de ter esse bound simples; seria
necessário decompor produtos e carries em limbs menores antes de estender o
circuito compensado.

## Step circuit CBD

A step circuit CBD usa `P=16`, `K=1..4`, `S=2^(16K)`, centro `0<=N<=S` e
raio `0<=R<=S`. Para `K=4` e `r_num<=40`, `r_den<=10`:

| Intermediário | Bound inteiro |
|---|---:|
| `N(S-N)` | `<2^128` |
| `r_num*N(S-N)` | `<2^134` |
| divisor do centro `r_den*S` | `<=10*2^64<2^68` |
| quociente do centro | `<=2^64` |
| resto do centro | `<10*2^64` |
| numerador do raio `r_num*R` | `<=40*2^64<2^70` |
| raio antes da saturação | `<40*2^64+2` |
| raio de saída | `<=2^64` |

O resto do centro é restringido por uma testemunha `gap` que prova
`remainder+gap+1=divisor`. O resto da propagação do raio é one-hot em
`{0,...,r_den-1}`. A saturação em `S` é selecionada pelos bits acima da
posição `16K`, impedindo que um raio maior que o domínio seja carregado no
estado. Todas as expressões permanecem abaixo do módulo escalar dos ciclos
Pallas/Vesta.

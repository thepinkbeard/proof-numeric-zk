# FP32 positivo-normal exato em R1CS

## Perfil

O baseline aceita `+0` e normais positivos. Um valor não nulo é representado
por expoente enviesado `E∈[1,254]` e significando explícito
`S∈[2^23,2^24-1]`:

\[
x=S\,2^{E-150}.
\]

Zero exige `E=S=0`. Negativos, subnormais, NaN e infinito não possuem
codificação válida. O mapa logístico restringe `x∈[0,1]` e `r∈[0,4]`.

## Multiplicação

Para operandos não nulos, o circuito verifica:

\[
P=S_aS_b,\qquad
D=2^{23+h},\qquad
P=QD+R,\quad 0\le R<D,
\]

onde `h` é o bit 47 de `P`. O expoente anterior ao carry é

\[
E_b'=E_a+E_b-127+h.
\]

O quociente `Q` é arredondado por nearest-ties-to-even usando a comparação de
`R` com `D/2`. Um eventual carry de 25 bits incrementa o expoente e desloca o
significando para a forma canônica.

## Subtração `1-x`

Todos os valores são alinhados temporariamente à escala exata `2^-149`:

\[
X=S_x2^{E_x-1},\qquad \Delta=2^{149}-X.
\]

O expoente normalizado testemunhado define `D=2^{E'-1}`. O circuito prova

\[
\Delta=QD+R,\quad 0\le R<D
\]

e aplica a mesma regra RNE. A potência `D` é construída por sete seleções
binárias sucessivas a partir dos bits do expoente; não há tabela nem um ramo
por expoente.

## Etapa logística e decisão

A ordem é exatamente:

1. `c = round_fp32(1-x)`;
2. `p = round_fp32(x*c)`;
3. `y = round_fp32(p*r)`.

O parâmetro `r` é fixado pelo circuito. A decisão `y≥0,5` é provada pelo
expoente: normais com `E≥126` satisfazem o limiar; zero e `E≤125` não.

O estado Nova contém contador, expoente, significando, bit de zero e decisão.
Witnesses não alteram a forma R1CS.

## Custo medido

| Relação | Constraints | Range checks |
|---|---:|---:|
| multiplicação | 474 | 417 |
| `1-x` | 924 | 855 |
| etapa completa e decisão | 1.721 | 1.551 |

Backend: Nova 0.73, Pallas/Vesta, Pedersen+IPA. O resultado bruto da comparação
CBD-32 × FP32 está em `results/fp32_cbd_go_no_go.json`.

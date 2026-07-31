# Certified Floating Dyadic (CFD) v1

## Escopo e perfil público

CFD é uma representação *IEEE-shaped* para provas iterativas. Ela preserva
sinal, expoente e significando normalizado, `+0` canônico e escolha honesta do
centro por round-to-nearest-ties-to-even (RNE), mas a relação provada é de
inclusão intervalar. Não há alegação de conformidade operacional IEEE 754.

O perfil público canônico contém

```text
CfdProfile(version, p, exponent_lanes, radius_bits, domain, operations,
           thresholds, rounding_policy, overflow_policy, next_profile,
           profile_hash)
```

`exponent_lanes` é uma lista ordenada e sem repetições. O hash SHA-256 cobre a
serialização canônica de todos os campos anteriores ao próprio hash. Domínio,
corredor, operações, limiares e política de fallback são, portanto, parte da
declaração provada.

## Valores e concretização

Um valor é `CertifiedFloat(sign, exponent_lane, significand, radius, zero,
wide)`. Para `zero=wide=0`, se a lane seleciona `e`, então

\[
2^{p-1}\le M<2^p,\qquad
c=(-1)^s M2^{e-(p-1)},\qquad
\operatorname{ulp}(e)=2^{e-(p-1)}
\]

e a concretização é

\[
\gamma(v)=[c-R\operatorname{ulp}(e),c+R\operatorname{ulp}(e)]\cap D.
\]

`zero=1` exige a codificação canônica `(lane,M,R,wide)=(0,0,0,0)` e denota
`{0}`. `wide=1` exige `(lane,M,R,zero)=(0,0,0,0)` e denota todo o domínio `D`.
`wide` é absorvente e sua única decisão válida é `Indeterminate`.

A v1 aceita somente finitos normais e `+0`. Subnormais, NaN, infinitos, `-0`
e flags IEEE ficam fora do escopo. Perfis não negativos fixam `sign=0`, embora
o sinal permaneça na definição geral.

## Relação certificada

Para uma operação real `f` e estados de entrada `v`, uma witness de saída `w`
é válida quando é canônica, pertence ao mesmo perfil (ou a uma transição de
perfil autorizada) e

\[
f(\gamma(v))\cap D\subseteq\gamma(w).
\]

O algoritmo honesto calcula o centro por RNE e escolhe o menor raio inteiro em
ULPs que contém o bound racional. O circuito não exige unicidade desse centro.
Qualquer centro alternativo ainda precisa satisfazer a inclusão e, portanto,
só pode preservar ou reduzir disponibilidade. Se o corredor ou o campo do raio
não comportar o resultado, a única saída válida é `wide`; truncagem silenciosa
é proibida.

As operações implementadas são `add`, `mul`, `mul_public`, `one_minus`, `fma`,
a recorrência afim estável e o mapa logístico. A topologia sequencial normaliza
nas mesmas fronteiras do FP32; a topologia fusionada/checkpoint normaliza uma
vez por passo e é apenas uma ablação.

## Decisão trivalorada

Para limiar público `t`,

\[
\operatorname{decide}(v,t)=
\begin{cases}
\textsf{True} & \inf\gamma(v)\ge t,\\
\textsf{False} & \sup\gamma(v)<t,\\
\textsf{Indeterminate} & \text{caso contrário.}
\end{cases}
\]

A assimetria implementa o predicado `x >= t`: igualdade pertence ao ramo
verdadeiro somente quando todo o intervalo está acima ou sobre o limiar.

## Teorema de segurança iterativa

**Teorema.** Seja `x[0]` a referência exata e `v[0]` um estado CFD tal que
`x[0] in gamma(v[0])`. Se, em cada fold `n`, a relação de transição prova
`f[n](gamma(v[n])) subseteq gamma(v[n+1])`, então, para todo `n`,
`x[n] in gamma(v[n])`. Toda decisão `True` ou `False` coincide com o predicado
da referência; cruzamentos do limiar resultam em `Indeterminate`.

**Prova.** A inclusão da trajetória segue por indução. A base é a hipótese
inicial. No passo, `x[n] in gamma(v[n])` implica
`x[n+1]=f[n](x[n]) in f[n](gamma(v[n]))`, que está incluído em
`gamma(v[n+1])`. Se o limite inferior é pelo menos `t`, toda concretização,
inclusive `x[n]`, satisfaz o predicado. Se o limite superior é menor que `t`,
nenhuma o satisfaz. Os demais casos não autorizam decisão definitiva.

## Correspondência binary32

Para `p=24`, raio zero e lanes que contenham o expoente não enviesado, a função

```text
(binary32 positivo normal) <-> (sign=0, e, M, radius=0, zero=0, wide=0)
+0                         <-> (zero=1)
```

é bijetiva. O significando explícito `M` é o bit implícito seguido dos 23 bits
de fração. Essa é uma equivalência de representação, não de semântica
operacional: operações CFD continuam certificando conjuntos.

## Aritmetização R1CS v1

Expoentes são selecionados por one-hot em corredores públicos. Centros e raios
são convertidos para uma escala diádica comum; desigualdades são provadas por
gaps não negativos com range checks. Bits de `zero`, `wide` e decisão são
canônicos, e a forma do circuito não depende da witness. O estado Nova tem sete
elementos: contador, lane, significando, raio, zero, wide e decisão.

O compromisso de perfil é atualmente SHA-256 sobre a serialização canônica e
é registrado junto às métricas. Vincular uma reprova a um perfil seguinte e
medir separadamente o custo desse vínculo permanece trabalho experimental
pendente; não deve ser alegado como concluído.

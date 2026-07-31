# Especificação matemática

Para parâmetros `P` e `K`, sejam

\[
B=2^P,\qquad S=B^K=2^{PK}.
\]

O numerador interior é

\[
N=\sum_{i=0}^{K-1}a_iB^{K-1-i},\qquad 0\le a_i<B.
\]

A representação canônica de `[0,1]` contém `K` limbs e um bit `is_one`:

\[
\operatorname{value}(a,is\_one)=
\begin{cases}
1,&is\_one=1,\\
N/S,&is\_one=0.
\end{cases}
\]

Quando `is_one=1`, todos os limbs devem ser zero. Portanto zero e um têm
representações únicas. Valores interiores possuem `P*K` bits fracionários e
ULP `2^(-P*K)`.

## Quantização

Para racional `x in [0,1]`, escreva

\[
xS=q+\rho,\qquad q=\lfloor xS\rfloor,\quad 0\le\rho<1.
\]

Truncagem escolhe `q`. Arredondamento para o mais próximo escolhe `q+1` quando
`rho > 1/2`; no empate, ties-to-even usa a paridade de `q`, e ties-away escolhe
`q+1`. O erro retornado pela API é sempre `quantizado - x`, como racional exato.

## Multiplicação

Para valores interiores:

\[
A=N_xN_y=Sq+d,\qquad 0\le d<S.
\]

O quociente `q` é o resultado truncado e `d` é o resto descartado. O modo de
arredondamento pode incrementar `q`; se o resultado for `S`, ele é
canonicalizado como o valor especial um.

Operacionalmente, `A` é obtido por convolução dos limbs. Testes verificam que a
reconstrução por convolução coincide exatamente com `N_xN_y`.

Toda operação retorna overflow explicitamente. Nenhum wraparound de inteiro é
parte da semântica.

## Residual: alternativas analisadas

### A. Resto local da operação

Em cada multiplicação, manter `d` da relação `A=S*q+d` permite auditar a perda,
mas reutilizar simplesmente `d` na operação seguinte não preserva em geral a
expressão original: em uma composição não linear aparecem termos cruzados.
Essa alternativa será registrada como telemetria, não chamada de compensação
correta.

### B. Estado em grade fina

A variante inicial compensada mantém

\[
X=SV+E,\qquad x=X/S^2=V/S+E/S^2,
\]

com `0 <= X <= S²`. Após quantizar `X`, a normalização escolhe `V` próximo de
`X/S` e define exatamente `E=X-SV`, dando

\[
|E|\le S/2
\]

exceto pela escolha canônica nos endpoints, onde o mesmo bound continua
válido. Para o mapa logístico com `r=R/D`, antes da nova quantização:

\[
x'=\frac{R X(S^2-X)}{D S^4}.
\]

Não há divisão nativa no circuito: quociente e resto serão testemunhas ligados
por uma igualdade inteira e range checks. Essa variante equivale a uma grade
de `2*P*K` bits e portanto aumenta materialmente o estado; ela não será
apresentada como precisão gratuita.

## Certified Bounded Dyadic (CBD)

Um perfil CBD fixa publicamente

\[
\Pi=(id,version,F,W,[L,U],rounding,thresholds,next),
\]

onde `F` é o número de bits fracionários, `W` limita a magnitude da
mantissa, `[L,U]` é o domínio admissível e `next` identifica uma precisão
superior aprovada.

Um estado CBD contém uma mantissa assinada `M` e um raio não negativo `R`.
Sua denotação é

\[
\llbracket M,R\rrbracket_\Pi =
\left[(M-R)2^{-F},(M+R)2^{-F}\right]\cap[L,U].
\]

A interseção com o domínio é necessária nos endpoints. Uma quantização
centrada em zero com raio de uma ULP representa `[0,2^-F]` sem deslocar o
centro apenas para eliminar a extremidade negativa do intervalo simétrico
bruto.

### Quantização e canonicalidade

O centro usa round-to-nearest-ties-to-even. Dados o centro `c` e um intervalo
exato `[a,b]`, o raio canônico é

\[
R=\max\left(
\left\lceil(c-a)2^F\right\rceil_+,
\left\lceil(b-c)2^F\right\rceil_+
\right),
\]

onde termos negativos são substituídos por zero. Nenhum raio menor, mantendo
o mesmo centro, contém o intervalo.

### Operações

Soma e subtração usam os extremos usuais de intervalos. Multiplicação avalia
os quatro produtos dos extremos. `fma(a,b,c)` calcula o centro exato
`center(a)*center(b)+center(c)` e o arredonda uma única vez; os bounds usam o
produto intervalar seguido da soma.

Para o mapa logístico, a operação fusionada calcula

\[
c'=round_F(r c(1-c)).
\]

Os extremos são avaliados em `a` e `b`; quando `[a,b]` contém `1/2`, `r/4`
também é candidato ao máximo. A implementação inicial exige `r>=0` e
`[L,U]` contido em `[0,1]`.

A primeira step circuit usa um enclosure mais simples e determinístico. Como
`|d(x(1-x))/dx|<=1` em `[0,1]`, ela define

\[
R'=\min\left(S,
\left\lceil\frac{r_{num}R}{r_{den}}\right\rceil+
\mathbb{1}[d_{center}\ne0]\right),
\]

onde `d_center` é o resto da divisão usada para quantizar o novo centro. Esse
raio é sound, mas pode ser maior que o raio mínimo obtido pela avaliação
exata dos extremos. As duas variantes são mantidas separadas na API como
`logistic_step_fused` e `logistic_step_lipschitz`.

### Ablação sequencial local

Para separar o efeito da representação do efeito da fusão, a variante
sequencial quantiza após `x(1-x)` e após multiplicar por `r`. Para
`x=(N+d)/S`, `|d|<=R`, ela calcula:

\[
Q_p=\operatorname{RNE}\left(\frac{N(S-N)}{S}\right),
\qquad
R_p=\min\left(S,
\left\lceil\frac{R|S-2N|+R^2}{S}\right\rceil+q_p\right),
\]

\[
Q_o=\operatorname{RNE}\left(\frac{r_{num}Q_p}{r_{den}}\right),
\qquad
R_o=\min\left(S,
\left\lceil\frac{r_{num}R_p}{r_{den}}\right\rceil+q_o\right),
\]

onde `q_p` e `q_o` indicam divisão inexata. O complemento `1-x` é exato na
mesma grade diádica. A API correspondente é
`logistic_step_sequential_local_bound`.

Fusão e execução sequencial não induzem uma ordem total de larguras visíveis:
arredondamentos mudam os centros e a interseção com `[0,1]` pode tornar
qualquer uma aparentemente mais estreita em casos pontuais. A propriedade
exigida é inclusão da referência e ausência de decisões certificadas
incorretas; largura e cobertura são comparadas empiricamente.

### Predicado certificado

Para `x>=tau`,

\[
decision(x,\tau)=
\begin{cases}
True,&lower(x)\ge\tau,\\
False,&upper(x)<\tau,\\
Indeterminate,&\text{caso contrário}.
\end{cases}
\]

Somente `True` e `False` autorizam uma decisão externa. `Indeterminate` exige
nova prova sob um perfil aprovado de maior precisão e ligada à mesma entrada.

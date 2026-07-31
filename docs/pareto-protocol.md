# Protocolo da fronteira de Pareto certificada

Data de congelamento: 2026-07-30. Versão do esquema: 1.

Este documento fixa a unidade de comparação, as métricas, a matriz
experimental e o critério de dominância **antes** de qualquer medição da
rodada. Ele existe para impedir ajuste retroativo de perfis, limiares ou
domínios depois de observar cobertura.

## 1. Motivação

Até o registro [0016](progress/0016-certified-floating-dyadic.md) o experimento
mediu duas grandezas separadas: custo de circuito em constraints e cobertura em
decisões definitivas. Nenhuma das duas ordena as representações sozinha. CFD-24
custa 41,7% menos que o FP32 exato restrito e perde disponibilidade em regime
caótico; CBD-32 custa menos ainda e também abstém. O veredito resultante foi
"go parcial", que não é uma conclusão.

A causa é a unidade de comparação. Formatos não certificados sempre respondem,
inclusive quando respondem errado; formatos certificados nunca respondem errado,
mas às vezes não respondem. Comparar as duas famílias por constraints, por
precisão ou por cobertura isoladamente é uma comparação entre grandezas
diferentes.

O protocolo adota como unidade a **decisão definitiva correta** e trata
abstenção como um custo recuperável por reprova em perfil maior.

## 2. População de decisões

Uma configuração é a tupla

```text
(workload, representação, perfil, topologia, bound, limiar)
```

Para cada configuração, o experimento produz um conjunto de oportunidades de
decisão. Em workloads de trajetória, uma oportunidade é o par
`(estado n, limiar t)` para `n = 0..N` e `t` em um conjunto público de limiares.
Em workloads de decisão única, uma oportunidade é o par `(caso i, limiar t)`.

Cada oportunidade recebe exatamente um valor em `{True, False, Indeterminate}`.
Formatos não certificados nunca produzem `Indeterminate`: a decisão é o
predicado aplicado ao valor finito calculado.

Sejam:

- `N` o número total de oportunidades de decisão;
- `D` o número de decisões definitivas, isto é `True` ou `False`;
- `A = N - D` o número de abstenções;
- `X` o número de decisões definitivas que discordam da referência exata;
- `G = D - X` o número de decisões definitivas corretas.

A referência é racional exata quando o workload permite, e MPFR com precisão
`max(256, 2N + 128)` bits e bound de erro conservador nas trajetórias longas.
Quando a referência é MPFR, uma oportunidade cujo intervalo de confiança da
própria referência cruza o limiar é excluída de `N` e registrada em
`reference_inconclusive`; ela não pode ser contada nem como acerto nem como
erro.

`X` é chamada de decisão definitiva incorreta. Ela não implica falha de
soundness criptográfica: para FP32 e ponto fixo, `X > 0` significa apenas que a
aritmética finita decidiu um predicado sobre o número real de forma diferente da
referência exata. Para CBD e CFD, `X > 0` seria uma violação do teorema de
inclusão e é tratada como defeito, não como característica.

## 3. Métricas

### 3.1 Disponibilidade

```text
alpha = D / N
```

### 3.2 Taxa de decisão definitiva incorreta

```text
omega = X / D,  indefinida quando D = 0
```

### 3.3 Custo total do circuito

`C` é o número de constraints R1CS brutas da step circuit, medido por síntese em
`TestConstraintSystem` no mesmo backend Nova 0.73/Pallas-Vesta. É determinístico
e independente de máquina, ao contrário dos tempos.

O custo total de uma configuração é

```text
C_total = C * U
```

onde `U` é o número de invocações da step circuit: `N_steps` em workloads de
trajetória e `N_casos` em workloads de decisão única. O circuito Nova aumentado
é reportado como grandeza secundária, porque o overhead estrutural constante
domina instâncias pequenas e comprime diferenças reais entre representações.

### 3.4 Custo por decisão definitiva correta

Métrica primária da fronteira:

```text
kappa = C_total / G,  indefinida quando G = 0
```

`kappa` é comparável entre famílias porque penaliza simultaneamente as duas
formas de falhar: abstenção reduz `G` pelo numerador de oportunidades perdidas,
e decisão incorreta reduz `G` diretamente. Uma representação barata que erra
muito e uma representação barata que se abstém muito recebem ambas um `kappa`
alto, pelos motivos certos.

### 3.5 Custo esperado com escalonamento

Uma política de escalonamento de dois níveis `(p -> p')` prova no perfil `p`,
aceita as decisões definitivas e reprova apenas as oportunidades abstidas no
perfil `p'`, vinculada ao mesmo compromisso da entrada privada. O custo
amortizado por oportunidade é

```text
C_esc = C_p + (A_p / N) * (C_p' + L)
```

onde `L` é o custo em constraints do gadget que vincula as duas provas ao mesmo
compromisso. `L` deve ser **medido**, não estimado; enquanto o vínculo não
existir em circuito, o relatório registra `L = null` e `kappa_esc` fica marcada
como modelo, não como medida.

O número de decisões corretas após escalonamento é

```text
G_esc = G_p + G_(p'|A_p)
```

isto é, as corretas obtidas em `p` mais as corretas obtidas ao reprovar em `p'`
somente as oportunidades que `p` abstiveram. Então

```text
kappa_esc = (C_esc * N) / G_esc
```

Escalonamento não é definido para formatos não certificados: eles não abstêm,
logo `A = 0` e `kappa_esc = kappa`. Essa assimetria é uma propriedade da
representação, não um artefato da métrica.

### 3.6 Horizonte definitivo

Para workloads de trajetória, com limiar principal `t*`:

```text
H = maior n tal que toda oportunidade (m, t*) com m <= n é definitiva
```

`H` complementa `alpha` porque distingue uma configuração que decide os
primeiros 300 passos e depois satura de outra que decide 30% dos passos
espalhados. Registra-se também `first_abstention`, o menor `n` sem decisão
definitiva, e `first_wide` quando a representação possui estado absorvente.

## 4. Chave de junção entre custo e cobertura

Custo e cobertura são produzidos por binários distintos. A junção usa um
identificador canônico calculado de forma idêntica dos dois lados.

O perfil de experimento é serializado canonicamente na ordem abaixo, com
inteiros em big-endian e cada campo de tamanho variável precedido pelo seu
comprimento em `u32`:

```text
schema_version : u16
workload       : string
family         : string   // fixed-point | cbd | cfd | fp32
representation : string   // identificador canônico da variante
precision_bits : u32
bound_mode     : string   // none | global-lipschitz | local-taylor | interval
topology       : string   // fused | sequential | single-shot
exponent_lanes : [i16]
radius_bits    : u32
rounding       : string
parameters     : [(string, rational)]   // r, x0, alpha, innovation
thresholds     : [rational]
domain         : (rational, rational)
```

`profile_hash = SHA-256(serialização canônica)`, registrado em hexadecimal
minúsculo como `profile_hash_hex`. Racionais são serializados por sinal,
numerador e denominador em bytes big-endian, na mesma convenção já usada por
`CfdProfile::canonical_bytes`.

Toda linha de custo e toda linha de cobertura carregam `profile_hash_hex`. A
junção é um `inner join` por essa chave. Linhas sem par são reportadas
explicitamente e não são descartadas em silêncio.

## 5. Matriz experimental congelada

### 5.1 Workloads

| Workload | Tipo | Parâmetros | Oportunidades | Limiares |
|---|---|---|---|---|
| `logistic-map` | trajetória | `x0 = 1/10`, `r` em `{7/2, 37/10, 39/10, 4}` | 1.001 estados | `1/4`, `1/2`, `3/4`; principal `1/2` |
| `stable-affine` | trajetória | `x0 = 1/10`, `alpha = 1/2`, `innovation = 3/4` | 1.001 estados | `1`; principal `1` |
| `adult-logistic` | decisão única | modelo determinístico de 6 atributos numéricos | 1.857 casos | `0` |
| `ecef-proximity` | decisão única | corda quadrática, raio 100 km | 2.000 casos | corda² equivalente |

Os workloads de decisão única são o eixo de aplicação: metade dos casos ECEF é
gerada deliberadamente próxima do limiar, o que os torna o teste discriminante
entre decidir e decidir certo.

### 5.2 Representações

| Família | Variantes |
|---|---|
| ponto fixo | limbs `16×{1,2,3,4}`, arredondamento truncado e nearest-even |
| FP32 | positivo-normal exato restrito, `p = 24` |
| CBD | `{16,32,48,64}` bits fracionários × `{global-lipschitz, local-taylor, sequential-local}` |
| CFD | `p ∈ {16,24,32}` × corredores de `{5,9,17}` lanes × `{sequential, fused}` |

Ponto fixo `16×1` é Q16.16 e `16×2` é Q32.32 na ordem de operações usada; eles
não são tratados como formatos numericamente distintos.

### 5.3 Regimes declarados

A partição de regimes é declarada agora e não pode ser alterada depois:

- **estável**: `logistic-map` com `r = 7/2` e todo o `stable-affine`;
- **caótico**: `logistic-map` com `r` em `{37/10, 39/10, 4}`;
- **decisão única**: `adult-logistic` e `ecef-proximity`.

Conclusões por regime só são admissíveis para esses três blocos.

## 6. Critério de dominância

Para configurações `X` e `Y` no mesmo workload, `X` domina `Y` quando

```text
omega_X <= omega_Y  e  kappa_X <= kappa_Y  e  alpha_X >= alpha_Y
```

com pelo menos uma desigualdade estrita. A fronteira de Pareto é o conjunto de
configurações não dominadas.

A fronteira é reportada em duas regiões:

1. **região certificada**, com `omega = 0`. Só configurações desta região podem
   sustentar uma alegação de segurança de decisão.
2. **região não certificada**, com `omega > 0`. Configurações aqui podem ter
   `kappa` menor, e frequentemente terão; isso é o resultado, não um problema.
   Elas são reportadas na mesma tabela e explicitamente desqualificadas para
   qualquer alegação de segurança.

Uma alegação de vantagem exige dominância dentro da região certificada e no
regime declarado. Nenhuma alegação global de dominância é admissível a partir
desta matriz.

## 7. Regras de congelamento

1. Domínio, corredor de expoentes, `radius_bits` e limiares não podem ser
   ajustados depois de observar cobertura. Um perfil barato com perda excessiva
   de cobertura é preservado como resultado negativo.
2. A partição de regimes da seção 5.3 é fixa.
3. Toda configuração da matriz entra no relatório, inclusive as que não
   produzem nenhuma decisão definitiva. `kappa` indefinida é reportada como
   `null`, nunca omitida.
4. `kappa_esc` só é reportada como medida quando `L` for medido em circuito;
   caso contrário é rotulada como modelo.
5. Constraints são a grandeza determinística de custo. Tempos e memória são
   reportados separadamente, com repetições e intervalo de confiança, e não
   participam do critério de dominância.
6. Correções não editam registros concluídos; geram um novo registro numerado
   em `docs/progress/`.

## 8. Critérios de sucesso da rodada

A rodada é bem-sucedida quando produzir, para cada regime declarado:

- a fronteira de Pareto da região certificada;
- a identificação de qual família a compõe e a partir de qual perfil;
- a quantificação de `omega` das famílias não certificadas nos workloads de
  fronteira, especialmente ECEF;
- `H` e `first_abstention` por configuração de trajetória;
- `kappa_esc` medida para pelo menos uma cadeia `p -> p'`.

Um resultado onde nenhuma configuração certificada domine em algum regime é um
resultado válido e deve ser publicado como tal.

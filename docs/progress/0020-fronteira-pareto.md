# 0020 — Fronteira de Pareto

Data: 2026-07-30. Estado: custo e cobertura juntos por `profile_hash_hex`, com
veredito por regime. O "go parcial" de 0016 está resolvido.

## Objetivo e estado

Produzir a tabela que o capítulo afirma: qual representação domina em qual
regime, medida em custo por decisão correta, e não em constraints nem em
precisão isoladamente.

Duas coisas precisavam existir antes. A recorrência afim só tinha circuito CFD e
FP32, então o regime contrativo — aquele em que o formato não certificado
deveria ganhar — comparava certificado contra certificado. E não havia junção
entre os 694 resultados de custo e os 702 de cobertura.

## Arquivos modificados

- `crates/nova-circuit/src/affine.rs` (novo): testemunhas e circuitos da etapa
  afim em ponto fixo e CBD.
- `crates/nova-circuit/src/lib.rs`: `enforce_certified_decision` extraído como
  gadget compartilhado; módulo `affine` exportado.
- `crates/nova-circuit/src/main.rs`: `execute_affine`, `execute_cbd_affine`,
  validação dos parâmetros afins e liberação do workload afim para ponto fixo e
  CBD.
- `crates/experiment-runner/src/bin/frontier_eval.rs`: referência, trajetórias e
  matriz do workload afim nas quatro famílias.
- `scripts/run_pareto_sweep.sh` (novo): varredura de custo espelhando a matriz.
- `scripts/build_pareto.py` (novo): junção, fronteiras e figuras.
- `results/pareto_frontier.json`, `results/pareto_frontier.csv`,
  `results/pareto_logistic.svg`, `results/pareto_affine.svg` (novos).

## Decisões e hipóteses

1. O estado afim reaproveita o layout do logístico — `K` limbs mais um bit —
   reinterpretando o bit como parte alta em vez de sinalizador de `x == 1`. O
   domínio é `[0,2)` e o valor é `alto + N/S`. Um layout novo obrigaria a
   duplicar testemunha inicial, reconstrução e gadget de decisão.
2. A matriz afim é menor que a logística, e não por economia. `carry` decompõe
   o produto de duas variáveis, que a etapa afim não tem. Os três bound modes de
   CBD coincidem porque `alpha` **é** o fator de Lipschitz e é exato — não há
   Taylor a expandir. `compensated` calcula `x*(1-x)`. Emitir esses rótulos
   produziria pontos duplicados, com custo e cobertura idênticos, inflando a
   fronteira com repetição.
3. O raio afim é `ceil(alpha*raio) + 1`, ambos os arredondamentos para cima. Com
   `alpha < 1` o raio converge para `1/(1-alpha)` ULP em vez de crescer, e há
   teste fixando isso em 2 ULP para `alpha = 1/2`.
4. Dois valores de `alpha`: `1/2`, diádico, e `7/10`, não diádico. Com `alpha`
   diádico a etapa de ponto fixo não arredonda nada e o formato não certificado
   acerta por construção. Sem o segundo caso, o regime contrativo pareceria
   trivialmente favorável ao ponto fixo por uma propriedade do parâmetro em vez
   do regime.
5. **Uma fronteira por classe de garantia, nunca uma única.** Esta foi a decisão
   que mudou a conclusão da rodada, e está detalhada na seção de tentativas
   malsucedidas.
6. A varredura de custo roda com `--steps 2`. `raw_step_constraints` mede uma
   etapa e não depende do número delas; mil passos multiplicariam o tempo de
   prova sem mexer no eixo de custo.
7. `build_pareto.py` falha quando a junção cai abaixo de 80% ou quando dois
   perfis distintos colidem no mesmo hash. Uma junção quebrada produz uma
   fronteira pequena e plausível, que é pior que nenhuma.

## Comandos executados

```bash
cargo test -p nova-circuit --lib affine
scripts/run_pareto_sweep.sh
frontier-eval --steps 1000 --output results/frontier_coverage.json
python3 scripts/build_pareto.py
```

694 configurações de custo, 0 falhas, 20 min. 734 registros de cobertura, 21 s.
Junção: 694 pontos, 94,6%.

## Testes, métricas e evidências

**Custo da etapa afim**, `raw_step_constraints`, contra o logístico do mesmo
perfil:

| representação | afim | logístico | razão |
| --- | --- | --- | --- |
| standard p32 | 216 | 292 | 0,74 |
| standard p64 | 380 | — | — |
| cbd p32 | 416 | 709 | 0,59 |
| cfd sequencial p24 | 846 | 1.003 | 0,84 |
| fp32 p24 | 1.418 | 1.721 | 0,82 |

A etapa afim é mais barata em todas as famílias porque `alpha*x + c` é
combinação **linear** de `x`: não há produto de duas variáveis a decompor. A
queda é maior em CBD (41%) porque lá desaparecem também as três relações do
bound de Taylor.

**Os 40 órfãos da junção**, sobre 734 registros, têm duas causas, ambas
declaradas: 8 são registros FP32 com limiar que não é potência de dois — `3/4` no
logístico, `5/4` e `3/2` no afim, limitação de 0018, em que o gadget compara
expoentes enviesados — e 32 são Adult e ECEF, que não têm circuito em nenhuma
família. O script os lista por nome em vez de descartá-los em silêncio.

**Nenhuma violação de invariante** nas 702 linhas: representação certificada com
decisão incorreta, zero; não certificada com abstenção, zero.

**O caso afim no atrator.** Com `alpha = 1/2`, `c = 3/4`, o atrator é exatamente
`3/2` e a trajetória converge por baixo sem nunca alcançá-lo — `x >= 3/2` é
falso em todos os 1.001 passos. Limiar `3/2`, decisões incorretas:

| representação | p16 | p32 | p48 | p64 |
| --- | --- | --- | --- | --- |
| standard, truncagem | 0 | 0 | 0 | 0 |
| standard, par mais próximo | 983 | 967 | 951 | 935 |
| fp32 exato | 976 | — | — | — |
| CBD e CFD | 0 | 0 | 0 | 0 |

O arredondamento para o mais próximo converte "aproxima-se de 3/2 sem alcançar"
em "é igual a 3/2", e a decisão inverte a partir daí. Mais precisão só adianta o
ponto de colapso em cerca de 16 passos por 16 bits. A truncagem acerta tudo, e é
importante dizer por quê: ela nunca arredonda para cima e a trajetória se
aproxima **por baixo**. Se o atrator fosse alcançado por cima, a truncagem
falharia e o par mais próximo acertaria. Nenhum dos dois tem garantia; um teve
sorte com o sinal da aproximação.

O CBD abstém-se com horizonte linear na precisão — 15, 31, 47 e 63 passos para
p16 a p64 — que é exatamente quando a distância ao atrator cai abaixo do raio.

**A tabela de dominância.** Custo por decisão correta, limiar principal de cada
regime:

| workload | regime | limiar | não certificado + barato | erro | certificado + barato | disp. | prêmio |
| --- | --- | --- | --- | --- | --- | --- | --- |
| logístico | estável | 1/2 | standard p16 trunc, 0,15 | 0% | cbd local p16, 0,40 | 100% | 3x |
| logístico | caótico | 1/4 | standard p16 trunc, 0,17 | 0% | cbd local p64, 10,43 | 12% | 62x |
| logístico | caótico | 1/2 | **nenhum** | — | cbd local p64, 10,60 | 12% | — |
| logístico | caótico | 3/4 | **nenhum** | — | cbd local p64, 10,43 | 12% | — |
| afim | diádico | 1/1 | standard p16 trunc, 0,11 | 0% | cbd local p16, 0,25 | 100% | 2x |
| afim | diádico | 3/2 | standard p16 trunc, 0,11 | 0% | cbd local p64, 11,56 | 6% | 101x |
| afim | não diádico | 1/1 | standard p16 trunc, 0,12 | 0% | cbd local p16, 0,26 | 100% | 2x |

O veredito é por regime, como o protocolo previa, e tem três faixas:

- **Regime fácil** (logístico estável, afim longe do atrator): o prêmio de
  certificação é 2x a 3x e a disponibilidade fica em 100%. Certificar é seguro
  barato, não um custo proibitivo.
- **Regime marginal** (afim no atrator): existe configuração não certificada
  que acerta tudo, mas ela é uma entre duas, e a outra erra 983 de 1.001 com o
  mesmo formato e a mesma precisão. Quem escolhe o arredondamento decide, sem
  saber, entre 0% e 98% de erro.
- **Regime caótico** com limiar no interior do atrator (logístico `1/2` e
  `3/4`): **nenhuma** configuração não certificada acerta tudo, em nenhuma
  precisão até 64 bits. A única opção sem erro custa 62x mais por decisão
  correta e responde 12% das vezes.

A última linha é o resultado negativo publicável que 0016 antecipava: em caos de
horizonte longo, a computação verificável de decisões ou aceita erro ou aceita
abstenção de 88%. Não há terceira opção nesta matriz.

**Adult e ECEF nas quatro famílias.** Os dois workloads de decisão única saíram
do CBD isolado e passaram pelo mesmo esquema. Decisões incorretas:

| representação | Adult (1.857 casos) | ECEF (2.000 casos) |
| --- | --- | --- |
| standard p16, truncagem | 0 | 550 |
| standard p16, par mais próximo | 0 | **492** |
| standard p32, truncagem | 0 | 1 |
| standard p32, par mais próximo | 0 | 0 |
| standard p48 e p64 | 0 | 0 |
| fp32 exato | 0 | 0 |
| CBD e CFD, todos os perfis | 0 | 0 |

Adult é fácil para todos: as margens do modelo logístico são largas, e só CBD-16
(1 caso) e CFD (2 casos) se abstêm.

ECEF separa as famílias. O ponto fixo de 16 bits erra 492 de 2.000 ao par mais
próximo; o CBD-16 decide 838, todos corretos, e abstém-se nos 1.162 restantes.
Isso reproduz a cobertura de 0008 (837 definitivas, 1.163 indeterminadas) a um
caso de distância, e o desvio vem de esta rodada codificar a diferença `a − b`
exata antes de quantizar, enquanto 0008 codificava `a` e `b` separadamente e
subtraía em CBD — um raio de quantização em vez de dois.

O número que fecha o argumento: 0008 contou **494 casos em que o valor central do
CBD-16 teria contradito a referência**. Esta rodada mede, do outro lado, **492
decisões que o ponto fixo de fato erra** na mesma grade e no mesmo arredondamento.
São o mesmo fenômeno visto de dois ângulos: as decisões que a semântica
certificada converteu em `Indeterminate` são exatamente aquelas em que responder
teria significado errar.

## Tentativas malsucedidas

A primeira versão de `build_pareto.py` calculava **uma** fronteira de Pareto por
grupo, com os eixos custo por decisão correta e taxa de erro, misturando as
famílias. A tabela saiu assim:

```
stable-affine  diadico  3/2  | standard p16  0.11  0.0% erro | certificado: NENHUM
```

Ou seja: nenhuma alternativa certificada na fronteira, sugerindo que certificar
nunca compensa. A conclusão era um artefato da relação de ordem. `standard p16`
com truncagem tem taxa de erro nula **observada nesta trajetória** e custo menor,
então dominava todos os pontos certificados nos dois eixos e os eliminava.

O erro é tratar "não errou aqui" e "não erra nunca" como o mesmo valor de eixo.
São produtos diferentes: um vale para toda entrada e é o que o circuito prova, o
outro é uma observação sobre uma trajetória — e a linha de cima da mesma tabela
mostra o mesmo formato, na mesma precisão, errando 983 de 1.001 só por mudar o
arredondamento. A correção foi calcular uma fronteira por classe de garantia e
reportar a razão entre elas como `certification_premium`. Sem isso, a rodada
teria concluído o oposto do que os dados mostram.

Uma segunda tentativa usou precisão fixa de 512 bits na referência afim. A
distância ao atrator cai como `alpha^n` e, depois de uns 500 passos, some dentro
do erro da própria referência: a cauda inteira virava `reference_inconclusive`,
justamente onde a decisão é marginal e a comparação interessa. A precisão passou
a escalar com o número de passos.

## Limitações conhecidas

1. Adult e ECEF têm cobertura homogênea nas quatro famílias, mas **nenhuma delas
   tem circuito**: `nova-circuit` só conhece o mapa logístico e a recorrência
   afim. Sem custo medido, os 32 registros aparecem como órfãos na junção e não
   entram na fronteira custo × cobertura. A comparação deles é de cobertura
   apenas, e é assim que precisa ser citada. Fechar a lacuna exige uma família de
   circuitos de produto interno, que é trabalho de porte próprio.
2. O custo esperado com escalonamento continua **modelado**. `build_pareto.py`
   calcula `C_p + (A_p/N)*C_p'` com `C_p'` medido, mas o vínculo em circuito que
   amarra a reprova ao mesmo compromisso da entrada não existe: o número é um
   limite inferior do custo real.
3. A fronteira usa `raw_step_constraints`, determinístico. Tempos de prova
   entram nos registros mas não na dominância, porque as três repetições atuais
   não sustentam ordenação.
4. `standard` e `carry` produzem a mesma trajetória e diferem só em custo, então
   `carry` nunca aparece na fronteira do logístico. É correto e esperado, mas
   significa que a decomposição de carry não tem regime próprio.
5. A dominância é calculada por `(workload, regime, limiar)`. Não há agregação
   entre limiares: um ponto pode dominar em `1/2` e não em `3/4`, como de fato
   acontece no logístico caótico.

## Próximo passo

Medir o vínculo de escalonamento em circuito, para que o termo
`P(indeterminado) * custo(p')` deixe de ser modelo. Com abstenções de 88% a 94%
no regime caótico, ele domina a métrica final.

## Instrução de continuidade para outra IA

Não recalcule a fronteira com uma relação de dominância única sobre todos os
pontos. As classes certificada e não certificada têm de ser separadas antes da
dominância, pela razão registrada acima — a conclusão se inverte. Ao acrescentar
um workload, acrescente-o aos **dois** lados: `run_pareto_sweep.sh` e a matriz de
`frontier_eval.rs`. Um lado só não gera erro, apenas some da junção, e
`build_pareto.py` vai listá-lo como órfão.

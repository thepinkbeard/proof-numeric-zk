# A escolha da representação numérica como decisão de segurança em computação verificável

Capítulo de qualificação. Esqueleto de seções herdado de
[cfd-paper-draft.md](cfd-paper-draft.md); resultados de
[0017](progress/0017-protocolo-pareto.md) a
[0022](progress/0022-validacao-e-consolidacao.md).

---

## 1. O problema

Um circuito aritmético que decide `x ≥ t` precisa representar `x` de alguma
forma. Ponto fixo é barato e sempre responde. FP32 exato é mais caro, tem
semântica familiar, e também sempre responde. Representações certificadas
carregam um enclausuramento junto do valor, custam mais, e às vezes recusam-se a
responder.

A literatura de computação verificável compara essas opções por custo de
circuito. É a comparação errada, e este capítulo mostra por quê.

O argumento em uma frase: **um formato que sempre responde não é mais disponível
que um que às vezes se abstém — é menos honesto sobre quando está errado.** A
comparação justa não é constraints por passo, é custo por decisão *correta*,
contabilizando abstenção certificada e o escalonamento que a torna recuperável.

### 1.1 O caso que torna o problema concreto

Proximidade ECEF: dados dois pontos em coordenadas cartesianas centradas na
Terra, decidir se a distância ao quadrado é menor que um limiar. É um predicado
geoespacial comum, do tipo que apareceria numa prova de localização.

Raio de 100 km sobre uma Terra de 6.371 km, 2.000 casos, metade deles gerada
deliberadamente a poucas partes por milhão do limiar.

Em ponto fixo de 16 bits fracionários, **492 decisões saem erradas** com
arredondamento ao par mais próximo, e 550 com truncagem. A razão é dimensional,
não numérica: a corda ao quadrado no limiar vale cerca de `2,5e-4`, e os casos
concentrados perto do limiar diferem dele por menos que o passo da grade de 16
bits. Trinta e dois bits erram 1 com truncagem e nenhum ao par mais próximo. De
48 bits em diante a grade cobre o limiar com folga.

O ponto não é que 16 bits sejam poucos. É que **o circuito de 16 bits não tem
como sinalizar que são poucos.** Ele responde 2.000 vezes com a mesma confiança
aparente, e 492 dessas respostas são falsas. O verificador aceita a prova: a
prova está correta, o circuito computou exatamente o que disse que ia computar.
O que estava errado era a pergunta que o circuito conseguia responder.

Na mesma grade de 16 bits, CBD decide 838 casos, todos corretos, e abstém-se nos
1.162 restantes. As 492 decisões que o ponto fixo erra caem inteiramente dentro
dessas abstenções — é o mesmo fenômeno que a rodada 0008 já havia registrado
pelo outro lado, contando **494 casos em que o valor central do CBD-16, se
tivesse sido usado como resposta, teria contradito a referência exata**. A
semântica certificada converteu todos em `Indeterminate` sem autorizar um único
erro.

Essa é a diferença que o capítulo defende, e ela não aparece em nenhuma métrica
de custo de circuito. Os dois formatos têm a mesma precisão e a mesma grade. O
que muda é que um sabe quando não sabe.

---

## 2. Métrica

Definida em [pareto-protocol.md](pareto-protocol.md) **antes** de qualquer
execução, para que nenhum perfil pudesse ser ajustado retroativamente.

Para cada configuração `(workload, representação, perfil, limiar)`:

- `C`: raw step constraints, determinístico e comparável entre máquinas
- `D`: decisões definitivas; `W`: definitivas **incorretas** contra referência racional/MPFR
- `A`: abstenções (`Indeterminate` ou `wide`)
- **Custo por decisão correta**: `C / (D − W)`
- **Custo esperado com escalonamento**: `C_p + (A_p/N) · (C_link + C_p')`
- **Horizonte definitivo**: primeiro passo sem decisão definitiva

O termo que carrega o argumento é `W`. Para FP32 e ponto fixo, `A = 0` e `D = N`
por construção: eles sempre respondem. Sem `W` no denominador, a comparação
premia exatamente o comportamento que o capítulo identifica como perigoso.

### 2.1 Duas classes de garantia, não uma

Um ponto de método que muda as conclusões. Ponto fixo com precisão suficiente
pode ter `W = 0` num workload — e então domina qualquer certificado, sendo mais
barato e igualmente correto.

Mas `W = 0` significa coisas diferentes nos dois casos. Para o certificado é
**garantido por construção**: o teorema de enclausuramento diz que nenhuma
entrada produz decisão definitiva incorreta. Para o ponto fixo é **observado**:
nenhuma das entradas *testadas* produziu erro. A primeira afirmação vale para
entradas futuras; a segunda, não.

Tratar as duas como equivalentes faria o ponto fixo dominar toda a fronteira e
esconderia o objeto de estudo. A fronteira é portanto calculada em duas classes
separadas — certificada e observada — e a razão entre a mais barata de cada uma
é reportada como **prêmio de certificação**.

---

## 3. Método experimental

Quatro workloads: mapa logístico (regime caótico `r = 3,7` e estável `r = 2,8`),
recorrência afim estável (`α` diádico e não diádico), regressão logística sobre
UCI Adult (1.857 casos), proximidade ECEF (2.000 casos).

Quatro famílias: ponto fixo em limbs de 16 bits, CBD em três modos de bound, CFD
em duas topologias, FP32 exato restrito.

### 3.1 Por que o custo é medido em constraints, e não em tempo

A escolha foi fixada no protocolo antes de executar, e depois verificada por
medição. Trinta rodadas intercaladas por representação, com comparação pareada
dentro da rodada e intervalo bootstrap: **12 de 60 comparações separam**, e onde
separam a magnitude é de milissegundos.

A razão está no overhead estrutural do Nova. O circuito de etapa é de 2,5%
(ponto fixo) a 13,1% (FP32) do circuito aumentado; os outros ~11.376 constraints
são idênticos entre representações por construção. Uma diferença de 5,9× no
circuito de etapa vira 1,12× no total, e 1,12× desaparece sob 4% de ruído de
máquina.

Tempo a 10 folds mede predominantemente o que não varia. Constraints medem o que
varia. A fronteira usa constraints por isso, não por conveniência.

**Junção por hash de perfil.** O bloqueio estrutural que impedia a fronteira era
que custo e cobertura vinham de binários diferentes, com esquemas JSON
incompatíveis e sem chave comum — o rótulo interno `cbd_local_taylor` nem
coincidia com o nome de CLI `cbd`. A solução foi um `ExperimentProfile` canônico
em `numeric-core`, com serialização determinística e `profile_hash_hex` emitido
pelos dois lados. Custo e cobertura passaram a se juntar por igualdade de hash,
não por convenção de nome.

O resultado: 694 configurações de custo, 734 registros de cobertura, 694 pontos
juntos, 94,6%. Os 40 órfãos são declarados nominalmente, não descartados: 8 são
FP32 com limiar que não é potência de dois, limitação conhecida do gadget de
decisão, e 32 são Adult e ECEF, que têm cobertura nas quatro famílias mas nenhum
circuito — a comparação deles é de cobertura apenas.

---

## 4. Soundness

### 4.1 Enumeração exaustiva em formato reduzido

O resto do projeto testa soundness por amostragem. Amostragem não demonstra
propriedade universal: mostra ausência de contraexemplo na amostra.

A saída é encolher o formato até a enumeração completa caber. Com 6 bits
fracionários, o conjunto de enclausuramentos CBD representáveis tem algumas
centenas de elementos, e dá para percorrer todos.

Restava o segundo quantificador — "para todo real *dentro* do enclausuramento" —
que é contínuo e não se enumera. O que o fecha é o mapa logístico ser contínuo e
unimodal em `[0,1]`, com único ponto crítico em `1/2`: a imagem de `[a,b]` é
exatamente `[min(f(a),f(b)), max(f(a), f(b), f(1/2) se 1/2 ∈ [a,b])]`. Conferir
esses extremos exatos é verificação completa do contínuo, não discretização fina
dele.

Resultado: **nenhum contraexemplo**, sobre todos os enclausuramentos
representáveis, cinco variantes de passo CBD, duas topologias CFD, e oito
iterações encadeadas.

### 4.2 O mesmo formato, sem certificado

O contraste, na mesma grade de 6 bits e nas mesmas 8 iterações:

```
ponto fixo reduzido: 10 de 65 decisões incorretas
```

O argumento do capítulo, reduzido a um caso que cabe inteiro na cabeça e é
verificável por enumeração completa. Mesma precisão, mesmas iterações: o
certificado nunca erra e às vezes se abstém; o ponto fixo sempre responde e erra
15% das vezes. A diferença não é precisão. É saber quando não sabe.

### 4.3 Validação diferencial

O baseline FP32 foi validado contra Berkeley SoftFloat, a implementação de
referência do IEEE 754, em host x86_64 Linux. Mais de 150.000 amostras nos dois
workloads mais 8 casos de arredondamento difícil fixos, comparando os três
intermediários de cada passo e não só a saída: coincidência bit a bit.

Esta validação estava pendente desde a rodada 0007 por um diagnóstico incompleto.
`softfloat-sys` decide as regras de build por `cfg!` num build script, e `cfg!`
ali avalia o **host**, não o alvo — de modo que nem Rosetta nem
`--target x86_64-apple-darwin` resolvem em Apple Silicon. Identificada a causa, a
correção foi rodar num host x86_64 Linux de verdade.

---

## 5. A fronteira

### 5.1 Regimes fáceis: o prêmio de certificação é de 2,2 a 2,6 vezes

| workload / regime | mais barato certificado | mais barato observado | prêmio |
| --- | --- | --- | --- |
| afim diádico | CBD-16, 254 | ponto fixo p16, 115 | 2,21× |
| afim não diádico | CBD-16, 262 | ponto fixo p16, 117 | 2,24× |
| logístico estável | CBD-16, 395 | ponto fixo p16, 150 | 2,63× |

Nos regimes contrativos, o ponto fixo de 16 bits acerta tudo, e o certificado
custa entre 2,2 e 2,6 vezes mais por decisão correta. **Esse é um resultado a
favor do ponto fixo**, e está reportado como tal.

A leitura que o capítulo defende não é que o prêmio seja pequeno, é que ele é o
preço de uma informação que o ponto fixo não fornece: você só sabe que 16 bits
bastavam *depois* de comparar com a referência exata. Se pudesse fazer isso em
produção, não precisaria da prova.

### 5.2 Regime caótico: o prêmio não é definível

| limiar | certificado | observado sound | prêmio |
| --- | --- | --- | --- |
| 1/2 | CBD-64, 10,60/correta, disponibilidade 12% | **nenhum** | — |
| 3/4 | CBD-64, 10,43/correta, disponibilidade 12% | **nenhum** | — |
| 1/4 | CBD-64, 10,43/correta | ponto fixo p16, 0,169/correta | 61,8× |

Nos limiares `1/2` e `3/4` do regime caótico, **nenhuma configuração não
certificada é sound**. Ponto fixo de 16 bits erra 34% das decisões. Não há
prêmio de certificação a calcular porque não há alternativa correta a comparar: o
certificado é a única opção que não erra, e custa 10,6 constraints por decisão
correta com 12% de disponibilidade.

O limiar `1/4` é o contraste instrutivo. Ali o ponto fixo é sound, e o prêmio
salta para 61,8×. A diferença entre 61,8× e "indefinível" não está na
representação — é a mesma — está em quão perto do limiar a trajetória passa. Em
`1/4` a órbita caótica raramente encosta no limiar, e o erro de arredondamento
não vira erro de decisão. Em `1/2` ela cruza o tempo todo.

**O prêmio de certificação não é uma constante da representação. É uma função de
quão perto do limiar o problema chega** — e essa é uma quantidade que o projetista
do sistema tipicamente não conhece de antemão. É esse desconhecimento que o
certificado precifica.

### 5.3 O resultado negativo

Em caos de horizonte longo, a fronteira não oferece terceira opção: ou se aceita
erro, ou se aceita abstenção de 88%. CBD-64 é a única representação sound, e
decide 12% das oportunidades. Corredores de expoente sozinhos não controlam o
crescimento da incerteza.

Isto foi antecipado em 0016 e está preservado como resultado, não como ressalva.
Se a aplicação for de horizonte longo em regime caótico, a resposta desta
fronteira é que a computação verificável de decisões ainda não tem solução
viável — e saber disso é mais útil que um número otimista.

---

## 6. Escalonamento: da abstenção à recuperação

Abstenção só é aceitável se houver caminho de volta: reprovar com perfil maior. E
esse caminho só vale alguma coisa se a segunda prova usar a **mesma entrada** da
primeira.

Sem esse vínculo, um provador que recebe `Indeterminate` pode escolher outra
entrada — uma que decida do jeito que lhe convém — e apresentar a prova do perfil
maior como se fosse a continuação da primeira. A abstenção deixaria de ser um
pedido de mais precisão e viraria uma oportunidade de troca. O argumento de
segurança da seção 1.1 dependia de um passo que, até a rodada 0021, não estava
implementado.

O circuito de vínculo prova três coisas: que o compromisso público é o Poseidon
dos limbs da entrada privada; que o estado inicial em `p` é a quantização correta
desses limbs; e que o estado inicial em `p'` é a quantização correta dos
**mesmos** limbs. Dois quocientes, um único dividendo — e é daí que vem a
impossibilidade da troca.

### 6.1 O custo do vínculo domina os perfis baratos

| p → p' | vínculo | compromisso | quantização |
| --- | --- | --- | --- |
| 16 → 32 | 1.280 | 1.076 | 204 |
| 32 → 48 | 1.280 | 1.076 | 204 |
| 48 → 64 | 1.281 | 1.076 | 205 |

O vínculo é plano na cadeia, e Poseidon responde por 84% dele. Substituindo o
termo modelado pelo medido:

| ponto | C_p | esperado | fator | abstenção |
| --- | --- | --- | --- | --- |
| CBD-16 afim | 254 | 1.924 | 7,6× | 98% |
| CBD-16 Lipschitz logístico | 336 | 2.208 | 6,6× | 99% |

Um passo de CBD-16 custa 254 constraints; o vínculo que autoriza escalar custa
1.281, cinco vezes mais. **Quando a abstenção é quase total, o custo real não
está em reprovar — está em ter o direito de reprovar.**

Isto reverte parte da leitura da seção 5.1. CBD-16 aparece lá como o certificado
mais barato, e é, para quem nunca precisa escalar. Quem escala 98% das vezes paga
7,6× o anunciado, e nesse ponto o prêmio de 2,2× vira algo próximo de 17×. O
número honesto depende de qual regime a aplicação habita, e a fronteira agora
carrega os dois.

---

## 7. Ameaças à validade

1. **Adult e ECEF não têm circuito.** As quatro famílias têm cobertura homogênea
   nesses workloads, mas `nova-circuit` só implementa o mapa logístico e a
   recorrência afim. A comparação de decisão única — que é o caso mais favorável
   ao certificado — é de cobertura, não de custo. Fechar isso exige uma família
   de circuitos de produto interno.
2. **A enumeração exaustiva vale para 6 bits.** É demonstração de que a
   construção é sound, não prova para p=16 ou p=64. Para esses vale o argumento
   analítico mais os testes amostrados.
3. **Berkeley cobre a referência, não o gadget R1CS.** A cadeia fecha por
   transitividade — o gadget é testado contra a referência —, mas não há um teste
   único que vá do circuito a Berkeley.
4. **O escalonamento é somado analiticamente.** O vínculo é medido isoladamente e
   somado ao custo de etapa; não há execução Nova de ponta a ponta que detecte a
   abstenção e dispare a segunda prova.
5. **Identificadores de perfil são verificados fora do circuito.** Recomputar o
   `profile_hash` em circuito custaria mais que o vínculo inteiro.
6. **O limiar de decisão FP32 precisa ser potência de dois**, porque o gadget
   compara expoentes enviesados. Oito configurações ficam de fora por isso.
7. **Uma única curva e um único esquema de compromisso** (Pallas/Vesta, Pedersen
   + IPA). Custo em constraints é determinístico e portável; tempo não é.

---

## 8. Contribuição

1. Reformulação da comparação de representações numéricas em computação
   verificável de custo de circuito para **custo esperado por decisão correta**,
   com abstenção e escalonamento contabilizados. Nenhum dos trabalhos de
   referência — Garg, Ernstberger, Riasi — formula custo dessa forma.
2. Separação metodológica entre soundness **garantida** e **observada**, sem a
   qual formatos não certificados dominam a fronteira por construção.
3. Vínculo de escalonamento em circuito, com custo medido, que transforma
   abstenção em recuperação e mostra que o vínculo domina o custo nos perfis
   baratos.
4. Soundness por enumeração exaustiva em formato reduzido, mais forte e mais
   barata que fuzzing.
5. A fronteira medida em quatro workloads e quatro famílias, com o achado ECEF —
   492 decisões erradas do ponto fixo convertidas em abstenção certificada na
   mesma grade — como argumento de segurança, e o colapso em caos como resultado
   negativo preservado.

---

## 9. Trabalho restante para submissão

- Circuitos de produto interno para Adult e ECEF, fechando a lacuna de custo
  nos workloads de decisão única.
- Execução Nova de ponta a ponta do escalonamento.
- Trabalhos relacionados com citações primárias.
- Raios relativos ou logarítmicos, se a aplicação alvo for de horizonte longo —
  a fronteira mostra que é ali que o crescimento de incerteza vincula.

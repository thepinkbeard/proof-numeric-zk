# 0019 — Harness unificado de cobertura

Data: 2026-07-30. Estado: cobertura das nove representações num esquema único,
junção com o lado de custo verificada ponta a ponta.

## Objetivo e estado

Fechar o bloqueio 2 do plano. A cobertura vinha de três caminhos com três
esquemas incompatíveis: `experiment-runner/src/main.rs` (`definite_decisions_*`,
sem CFD), `cfd_eval.rs` (`definite_025/050/075/100`, só CFD) e `workload_eval.rs`
(`definite`, só CBD). Nenhum emitia chave de junção, e o rótulo interno nem
coincidia com o nome de CLI. Comparar representações exigia alinhar esquemas na
mão, que é exatamente onde um erro de alinhamento passa despercebido.

O binário novo roda o mesmo workload nas nove representações e emite um registro
por `(representação, perfil, limiar)` com os mesmos campos para todas, incluindo
`profile_hash_hex`. Com 0018, os dois lados da fronteira passam a falar a mesma
chave.

## Arquivos modificados

- `crates/experiment-runner/src/bin/frontier_eval.rs` (novo): varredura,
  trajetórias, pontuação e esquema de saída.
- `crates/experiment-runner/Cargo.toml`: alvo `frontier-eval`.
- `crates/numeric-core/src/frontier.rs`: teste de hashes fixos.
- `results/frontier_coverage.json` (novo): 588 registros, 1.000 passos.

## Decisões e hipóteses

1. As trajetórias são calculadas pelas **funções de witness dos circuitos**
   (`StepWitness`, `CbdStepWitness`, `CompensatedStepWitness`, `CfdStepWitness`),
   não por uma reimplementação da aritmética. Uma reimplementação divergiria do
   circuito medido em 0018, e a fronteira estaria juntando o custo de um programa
   com a cobertura de outro. É a mesma razão da decisão 1 de 0018, aplicada ao
   outro lado.
2. A referência é MPFR com `2*passos + 128` bits, não aritmética racional exata.
   O racional exato no mapa logístico dobra o número de bits a cada passo; em
   1.000 passos o denominador é intratável. A precisão escolhida excede a
   divergência acumulada por margem larga, e o intervalo da referência é
   comparado ao limiar antes de julgar qualquer decisão.
3. Quando o intervalo da própria referência cruza o limiar, a oportunidade é
   contada em `reference_inconclusive` e **não** entra em `opportunities`. Sem
   isso, uma decisão que nem a referência consegue julgar contaria como erro do
   formato avaliado.
4. `unsound_definite` só é contado para formatos não certificados; para os
   certificados o campo mede violação de invariante e deve ser sempre zero. Os
   dois casos são registrados em campos separados (`unsound_definite` e
   `enclosure_violations`) porque significam coisas diferentes: um é o custo
   esperado de responder sempre, o outro seria um defeito de implementação.
5. Um único `x0 = 1/10` por regime, com quatro valores de `r`. A varredura é
   sobre representações e perfis, que é o eixo da fronteira; multiplicar por
   condições iniciais aumentaria o volume sem mudar a ordenação.
6. Os hashes verificados na junção ficaram fixados como teste em
   `numeric-core/src/frontier.rs`. Uma mudança no esquema do perfil manteria os
   dois lados concordando entre si com chaves novas, e os resultados de custo já
   gravados deixariam de juntar-se sem nenhum erro visível.

## Comandos executados

```bash
cargo build --release -p experiment-runner --bin frontier-eval
frontier-eval --steps 1000 --output results/frontier_coverage.json
cargo test -p numeric-core frontier
```

588 registros: 49 variantes × 3 limiares × 4 regimes. 2,4 s de execução.

## Testes, métricas e evidências

**Junção verificada.** Dez configurações foram executadas nos dois binários e os
`profile_hash_hex` comparados. Dez de dez coincidem, incluindo os dois corredores
CFD, que precisam produzir chaves distintas:

| configuração | hash (12 primeiros) | constraints | junta |
| --- | --- | --- | --- |
| standard p32 | `b7077c283939` | 292 | sim |
| carry p32 | `337558e57fd2` | 469 | sim |
| compensated p32 | `bb649c9de5c9` | 702 | sim |
| cbd-global p32 | `940571474d02` | 606 | sim |
| cbd p32 | `264d34141faf` | 709 | sim |
| cbd-sequential p32 | `7d94880a2547` | 782 | sim |
| cfd-fused p24 corredor -8:0 | `27fe47e08b60` | 777 | sim |
| cfd p24 corredor -8:0 | `776550dd98cd` | 1.003 | sim |
| cfd p24 corredor -16:0 | `88325ded647f` | 1.246 | sim |
| fp32 p24 | `2ba3d32b43eb` | 1.721 | sim |

**Invariantes de solidez.** Nas 588 linhas, nenhuma representação certificada
registra decisão incorreta ou violação de enclosure, e nenhuma não certificada
registra abstenção. São as duas propriedades que a semântica afirma, agora
medidas em vez de assumidas.

**O resultado central.** Limiar `1/2`, arredondamento par mais próximo, 1.001
oportunidades, decisões **incorretas**:

| representação | r=3,5 | r=3,7 | r=3,9 | r=4 |
| --- | --- | --- | --- | --- |
| standard p16 | 0 | 341 | 477 | 509 |
| standard p32 | 0 | 303 | 478 | 475 |
| standard p48 | 0 | 281 | 459 | 473 |
| standard p64 | 0 | 320 | 481 | 440 |
| compensated p32 | 0 | 280 | 454 | 463 |
| fp32 exato p24 | 0 | 334 | 473 | 467 |
| CBD e CFD, todos os perfis | 0 | 0 | 0 | 0 |

No regime periódico todos acertam e o custo extra do certificado não compra
nada. No regime caótico os formatos que sempre respondem erram perto de metade
das decisões, com confiança total e sem sinal de erro.

O ponto mais forte da tabela é que a coluna não é monótona em precisão: em
r=3,7 o ponto fixo de 64 bits erra **mais** que o de 48 (320 contra 281), e em
r=3,9 erra mais que todos os outros. Não é ruído de medição, é a consequência de
comparar duas trajetórias divergentes: depois que o erro excede a distância ao
limiar, qual lado do limiar o valor calculado cai é arbitrário, e mais mantissa
apenas muda o sorteio. Precisão não é um eixo que compre correção neste regime —
só a abstenção certificada compra. O padrão se repete em `1/4` e `3/4`.

**O preço da abstenção.** No mesmo r=4, limiar `1/2`, decisões definitivas:

| representação | p16 | p32 | p48 | p64 |
| --- | --- | --- | --- | --- |
| cbd_lipschitz | 8 | 16 | 24 | 32 |
| cbd_local_taylor | 13 | 28 | 45 | 60 |
| cfd sequencial | 7 | 7 | 7 | — |
| cfd fusionada | 11 | 13 | 13 | — |

CBD converte precisão em horizonte de forma quase linear; a CFD sequencial
satura em 7 decisões e para de responder à precisão. O raio cresce mais rápido
que o ganho de mantissa, e a saturação é o mecanismo por trás do impasse de 0016.

**Corredor de expoentes.** No mapa logístico os três corredores dão cobertura
idêntica e custos diferentes (777, 1.003 e 1.246). As trajetórias ficam em
`[0,1]` e nunca alcançam os expoentes baixos do corredor largo. No espaço da
fronteira, o corredor é eixo de custo puro neste workload: os pontos de corredor
largo são dominados por construção.

## Tentativas malsucedidas

A primeira leitura dos resultados por `r` filtrou pela string `"4"` e devolveu
tabela vazia, porque `rational_text` normaliza para `num/den` e emite `"4/1"`.
Nenhuma mudança no código: a normalização é o comportamento correto e o filtro é
que estava errado. Fica o registro porque a mesma armadilha aparecerá nos
scripts de análise da fase seguinte.

A primeira verificação da junção rodou `nova-circuit` num laço de shell com
`IFS=:` ainda ativo, o que quebrou `--exponent-lanes=-8:0` em dois argumentos e
produziu falha de parsing atribuída, por um momento, à CLI. A invocação direta
funciona; o defeito era do laço.

## Limitações conhecidas

1. Só o mapa logístico nesta rodada. O workload afim entrou no mesmo binário
   ainda nesta sessão e está registrado em 0020; Adult e ECEF continuam de fora.
2. Ponto fixo e compensado usam `K` fixo por precisão (`16*K`); a matriz do
   protocolo pedia limbs `16×{1..4}` como eixo independente, o que a
   implementação atual colapsa no eixo de precisão.
3. `mean_interval_width` e `maximum_interval_width` são zero para formatos não
   certificados, por não haver enclosure. Comparações de largura entre famílias
   não fazem sentido e o script de fronteira precisa ignorar esses campos.
4. FP32 aparece com um único perfil (p24, limiar potência de dois em circuito).
   Fora de circuito os limiares `1/4` e `3/4` são avaliados, mas não têm custo
   correspondente medido.
5. O custo do escalonamento continua modelado. Com abstenções de 940 a 994 em
   1.000 no regime caótico, o termo `P(indeterminado) * custo(p')` domina a
   métrica final, e ele ainda não é medido.

## Próximo passo

Rodar as demais cargas — afim estável, Adult e ECEF — em todas as
representações, para que a fronteira não seja um resultado de um workload só.

## Instrução de continuidade para outra IA

Os registros de cobertura e de custo juntam-se por `profile_hash_hex` e por mais
nada. Não junte por rótulo de representação: `cfd_ieee_shaped_sequential` é o
mesmo rótulo em workloads diferentes, de propósito, e juntar por ele mistura
linhas de experimentos distintos. Ao acrescentar uma representação, acrescente-a
a `RepresentationId::ALL` em `numeric-core/src/frontier.rs` e deixe os dois
binários derivarem o perfil de `FrontierPoint::profile`; construir o perfil no
binário é o que a rodada evitou.

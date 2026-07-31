# 0021 — Escalonamento em circuito

Data: 2026-07-30. Estado: o vínculo existe, está medido, e o resultado muda o
custo esperado dos perfis baratos.

## Objetivo e estado

Fechar a pendência mais antiga do projeto, aberta em [0009](0009-auditoria-da-rodada.md)
item 3: `next_profile()` em `numeric-core/src/cbd.rs` e `escalate_exact` em
`cfd.rs` existiam sem nenhum uso em circuito. Enquanto isso valesse, o termo
`P(indeterminado) * custo(p')` do [protocolo](../pareto-protocol.md) era hipótese,
e a defesa da abstenção certificada tinha um buraco no meio.

O buraco é este. Uma representação certificada que se abstém está pedindo mais
precisão, e a recuperação é reprovar com um perfil maior. Só que, sem vínculo,
nada obriga a segunda prova a usar a mesma entrada da primeira. Um provador que
recebe `Indeterminate` pode escolher outra entrada — uma que decida do jeito que
lhe convém — e apresentar a prova do perfil maior como se fosse a continuação da
primeira. A abstenção deixaria de ser um pedido de mais precisão e viraria uma
oportunidade de troca. O argumento de segurança de 0020, que sustenta o capítulo
inteiro, dependia de um passo que não estava implementado.

## Arquivos modificados

- `crates/nova-circuit/src/escalation.rs` (novo): `EscalationWitness`,
  `EscalationLinkCircuit` e os cinco testes.
- `crates/nova-circuit/src/bin/escalation_cost.rs` (novo): medição isolada.
- `crates/nova-circuit/src/lib.rs`: `field_from_u128`, `range_check` e
  `allocate_ranged` promovidos a `pub(crate)`; módulo `escalation` registrado.
- `crates/nova-circuit/Cargo.toml`: binário `escalation-cost`.
- `scripts/build_pareto.py`: `load_link_cost` e o termo de escalonamento agora
  medido.
- `results/escalation_link_cost.json` (novo).

## Decisões e hipóteses

**O que o circuito prova.** Dado um compromisso público `C` e dois perfis, o
grosso `p` e o escalonado `p'`:

1. `C = Poseidon(limbs finos)`, com range check de 16 bits por limb — sem ele a
   reconstrução do valor não seria injetora e dois conjuntos distintos de limbs
   poderiam colidir no mesmo compromisso;
2. o estado inicial em `p` é a quantização correta desses limbs;
3. o estado inicial em `p'` é a quantização correta dos **mesmos** limbs.

A estrutura é essa: dois quocientes, um único dividendo. Cada estado satisfaz
`fino = estado * 2^deslocamento + resto` com `resto` sob range check, e as duas
relações citam a mesma variável `fino`. É daí que vem a impossibilidade de trocar
a entrada entre as provas.

**Truncagem, não par mais próximo.** O piso é a única quantização que se escreve
como quociente e resto sem um bit de arredondamento por perfil. O arredondamento
continua sendo responsabilidade do circuito de etapa, que recebe este estado
pronto.

**O que o circuito deliberadamente não prova.** Os identificadores de perfil não
são recomputados dentro do circuito. O `profile_hash` é serialização canônica
fora de corpo, e refazê-la em circuito custaria muito mais do que o vínculo
inteiro. O verificador confere os dois identificadores contra os perfis
declarados antes de aceitar a prova. É uma verificação de fora, e está escrita
como tal no módulo, não escondida.

**Poseidon como compromisso.** `PoseidonROCircuit` já vem em `nova-snark`, com as
mesmas constantes que o Nova usa no próprio circuito de verificação. O valor
esperado é derivado pelo gadget circuital sobre um sistema descartável, não por
uma implementação nativa paralela: derivar o esperado de um código diferente do
que o circuito executa reintroduziria exatamente a classe de divergência que o
`profile_hash` compartilhado eliminou entre custo e cobertura em 0019.

## Comandos executados

```
cargo test -p nova-circuit escalation
cargo run --release -p nova-circuit --bin escalation-cost
python3 scripts/build_pareto.py
```

## Testes, métricas e evidências

Cinco testes, e os dois que importam são negativos:

- `a_different_private_input_breaks_the_commitment`: trocar a entrada privada
  mantendo o `z` público publicado falha. É o ataque que o vínculo existe para
  impedir.
- `a_forged_escalated_state_breaks_the_quantization`: adulterar só o estado
  escalonado, mantendo o grosso intacto, também falha — os dois quocientes vêm
  do mesmo dividendo.

**Custo medido**, cadeia `16 → 32 → 48 → 64`, compromisso em grade de 64 bits:

| p → p' | vínculo | compromisso | quantização |
| --- | --- | --- | --- |
| 16 → 32 | 1.280 | 1.076 | 204 |
| 32 → 48 | 1.280 | 1.076 | 204 |
| 48 → 64 | 1.281 | 1.076 | 205 |

O vínculo é plano na cadeia: varia uma constraint entre o primeiro e o último
degrau. Isso porque o compromisso é sobre a grade fina, que não muda, e a
quantização é a mesma relação com deslocamentos diferentes. Poseidon responde por
84% do total.

**O resultado que muda a conta.** Com `C_link = 1.281` medido, o custo esperado
`C_p + (A_p/N) * (C_link + C_p')` passa a ser dominado pelo vínculo justamente
nos perfis mais baratos:

| ponto | C_p | esperado | fator | abstenção |
| --- | --- | --- | --- | --- |
| CBD-16 afim | 254 | 1.924 | 7,6× | 98% |
| CBD-16 Lipschitz logístico | 336 | 2.208 | 6,6× | 99% |

Um passo de CBD-16 custa 254 constraints; o vínculo que autoriza escalar custa
1.281, cinco vezes mais. Quando a abstenção é quase total, o custo real não está
em reprovar, está em ter o direito de reprovar. A leitura de 0020 — de que
CBD-16 é o ponto mais barato do regime — só vale para quem nunca precisa escalar.
Quem escala 98% das vezes paga 7,6× o anunciado, e a comparação com ponto fixo
muda de sinal.

474 dos 694 pontos da fronteira têm sucessor disponível e portanto termo de
escalonamento medido. Os outros 220 são os perfis de topo da cadeia, sem
`next_profile`: o termo continua omitido em vez de estimado.

## Tentativas malsucedidas

`PoseidonConstantsCircuit` não implementa `Debug`, o que derrubou o `derive` do
circuito. Não é contornável do lado de cá e não custa nada: o `Debug` saiu.

A primeira versão media o vínculo com `commitment()` calculado por uma esponja
nativa (`PoseidonRO`) enquanto o circuito usava a circuital (`PoseidonROCircuit`).
As duas concordam, mas manter dois caminhos para o mesmo valor é precisamente o
erro que 0019 corrigiu na junção. Passou a usar o gadget circuital dos dois lados.

## Limitações conhecidas

1. O vínculo é medido isoladamente e somado analiticamente ao custo de etapa. Não
   existe ainda uma execução Nova de ponta a ponta que faça a primeira prova,
   detecte a abstenção e dispare a segunda. O número está certo; o encadeamento
   é aritmética de fora.
2. A igualdade dos identificadores de perfil é verificação do verificador, não do
   circuito (ver Decisões).
3. A quantização é por truncagem. Um perfil que exigisse par mais próximo no
   estado inicial precisaria de um bit a mais de arredondamento por lado.
4. O compromisso é sobre uma grade fina fixa de 64 bits. Escalar acima de 64
   exigiria recomprometer, e aí o vínculo entre a grade antiga e a nova é outro
   problema.

## Próximo passo

A dívida de validação: Berkeley TestFloat em host x86_64 e enumeração exaustiva
em formato reduzido. Depois, a redação.

## Instrução de continuidade para outra IA

O vínculo está em `crates/nova-circuit/src/escalation.rs` e o custo em
`results/escalation_link_cost.json`. Se mexer no circuito, rode
`cargo run --release -p nova-circuit --bin escalation-cost` e depois
`python3 scripts/build_pareto.py`, nessa ordem: o script lê o JSON do vínculo e
recalcula `expected_cost_with_escalation` para os 474 pontos. Os dois testes
negativos de `escalation.rs` são o que garante que o vínculo vincula — se algum
deles passar a falhar por "circuito satisfeito", a propriedade de segurança
quebrou, não o teste.

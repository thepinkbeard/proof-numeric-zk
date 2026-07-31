# 0018 — Circuitos parametrizados e chave de junção

Data: 2026-07-30. Estado: eixos da matriz do protocolo abertos na CLI; custos de
referência preservados bit a bit.

## Objetivo e estado

Remover o bloqueio 1 do plano: precisão, corredor de expoentes, largura de raio,
limiar de decisão, `r`, `x0` e workload eram constantes espalhadas por
`main.rs`, `cfd.rs` e `fp32.rs`. Sem varrer esses eixos não existe fronteira,
apenas pontos isolados.

Junto disso, fechar o bloqueio 2 pela metade: o lado de custo passa a emitir
`profile_hash_hex`, a chave de junção definida na seção 4 do protocolo. O lado
de cobertura recebe a mesma chave no registro 0019.

## Arquivos modificados

- `crates/numeric-core/src/profile.rs` (novo): `ExperimentProfile`, builder e
  serialização canônica.
- `crates/numeric-core/src/lib.rs`: exporta o módulo novo.
- `crates/nova-circuit/src/main.rs`: reescrito em torno de `Config`.
- `crates/nova-circuit/src/lib.rs`: limiar público em CBD paralelo e sequencial.
- `crates/nova-circuit/src/cfd.rs`: limiar público na escala diádica comum;
  `circuit_decision` exposto.
- `crates/nova-circuit/src/fp32.rs`: `decision_at_least_power_of_two` unifica os
  dois gadgets de decisão anteriores.
- `crates/nova-circuit/tests/step_circuit.rs`: acompanha as assinaturas novas.

## Decisões e hipóteses

1. O limiar vive no **struct do circuito**, não na witness. Ele é parâmetro
   público do predicado; se entrasse na witness, um provador poderia escolher o
   limiar depois de ver o valor, e a decisão provada deixaria de significar o
   que o protocolo afirma. Essa foi a decisão de projeto mais relevante da
   rodada.
2. `ExperimentProfile` é uma estrutura nova em `numeric-core`, não uma extensão
   de `CfdProfile`. `CfdProfile` descreve aritmética; `ExperimentProfile`
   descreve uma configuração de experimento, incluindo workload e limiar, que
   não pertencem à aritmética. Misturá-las tornaria o hash sensível a campos que
   o circuito não observa.
3. FP32 só aceita limiar potência de dois. O gadget compara expoentes
   enviesados, o que é exato apenas nesse caso. Limiares como `3/4` continuam
   avaliáveis fora de circuito, e a diferença entra na cobertura, não no custo.
4. O corredor de expoentes CFD é validado na CLI com mensagem explícita. As
   mesmas condições existiam como `assert` dentro de `validate_profile`; numa
   varredura de matriz elas abortariam a execução no meio com panic.
5. O teto do corredor é função do workload: `0` para o mapa logístico, domínio
   `[0,1]`, e `1` para o afim estável, domínio `[0,2]`. Não é uma constante
   global.
6. A execução IVC virou uma função genérica `run_ivc`. As oito representações
   repetiam setup, prova e verificação; a repetição é onde divergências de
   medição entram sem serem notadas.

## Comandos executados

```bash
cargo test --workspace
cargo build --release -p nova-circuit
nova-circuit --representation <rep> --k <k> --steps 2 --rounding nearest-even
```

94 testes passam. A CLI expõe agora `--workload`, `--precision`,
`--exponent-lanes`, `--radius-bits`, `--threshold`, `--r`, `--x0`, `--alpha` e
`--innovation`, além das sete anteriores.

## Testes, métricas e evidências

Regressão de custo, `raw_step_constraints`, perfis padrão:

| representação | k | constraints | referência |
| --- | --- | --- | --- |
| standard | 2 | 221 | — |
| compensated | 2 | 567 | 0015 |
| cbd-global | 2 | 606 | 0015 |
| cbd | 2 | 709 | 0015 |
| cbd-sequential | 2 | 782 | 0015 |
| cfd-fused | 1 | 777 | 0016 |
| cfd | 1 | 1.003 | 0016 |
| fp32 | 1 | 1.721 | 0016 |

Todos os valores coincidem com os registros anteriores. A parametrização não
mexeu no custo dos perfis já medidos, que é a condição para que os números de
0015 e 0016 continuem citáveis.

Eixos novos respondem como esperado:

| configuração | constraints |
| --- | --- |
| standard k=4, precisão 64 | 389 |
| cbd k=1, precisão 16, limiar 3/4 | 417 |
| cbd-sequential k=3, precisão 48 | 1.092 |
| cfd p16, corredor -4:0, raio 16 | 688 |
| cfd p16, corredor -4:0, raio 24 | 712 |
| cfd-stable, afim estável | 846 |
| fp32-stable, afim estável | 1.418 |

O limiar `3/4` em CBD custa o mesmo que `1/2`: a comparação é contra um
numerador público qualquer, e não contra uma potência de dois. Isso confirma que
o eixo de limiar é livre de custo em CBD e restrito em FP32, assimetria que
importa para a fronteira.

Combinações inválidas produzem mensagem, não panic:

```
--representation cbd --k 2 --precision 24
Error: --precision 24 é incompatível com --k 2: ponto fixo e CBD usam limbs de
16 bits, logo a precisão é 32. Ajuste --k.

--representation cfd --k 1 --precision 62 --exponent-lanes=-8:0
Error: --precision 62 com expoente mínimo -8 exige 69 bits na escala diádica
comum, acima do limite 62; reduza a precisão ou encurte o corredor
```

## Tentativas malsucedidas

Primeira tentativa colocou `threshold_numerator` dentro de `CbdStepWitness` e
propagou a partir dela. Compilava e passava nos testes, mas quebrava a
soundness pela razão da decisão 1. Foi refeita movendo o campo para o struct do
circuito, o que obrigou a atualizar todos os pontos de construção em `main.rs` e
nos testes.

A passagem de `--exponent-lanes -4:0` sem `=` é interpretada pelo clap como
flag, não como valor. Não é corrigível pelo lado do programa; a documentação e
os scripts de varredura precisam usar `--exponent-lanes=-4:0`.

## Limitações conhecidas

1. `--precision` não é livre em ponto fixo e CBD: ela precisa coincidir com
   `16*k`, porque os limbs são de 16 bits. A flag existe para tornar o perfil
   explícito no hash, não para abrir um eixo novo.
2. O corredor CFD é sempre um intervalo contíguo `min:max`. Corredores esparsos
   são representáveis em `CfdProfile` mas não pela CLI.
3. `ExperimentProfile` ainda só é emitido pelo lado de custo. Enquanto o
   `frontier-eval` não existir, o hash não junta nada.
4. FP32 permanece restrito ao subconjunto já validado; a flag `--precision` é
   aceita apenas com valor 24.

## Próximo passo

Emitir o mesmo `profile_hash_hex` do lado de cobertura, com esquema normalizado
único para as quatro famílias. Registro 0019.

## Instrução de continuidade para outra IA

O hash de perfil é calculado por `ExperimentProfile::profile_hash_hex` em
`numeric-core`. Não reimplemente a serialização no lado de cobertura: importe o
mesmo builder, ou a junção falha silenciosamente com duas chaves distintas para
a mesma configuração. Ao acrescentar um campo ao perfil, incremente
`PROFILE_SCHEMA_VERSION`, senão dois esquemas diferentes colidem no mesmo hash.

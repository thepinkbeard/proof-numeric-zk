# 0017 — Protocolo da fronteira de Pareto

Data: 2026-07-30. Estado: protocolo congelado; nenhuma medição desta rodada foi
executada antes deste registro.

## Objetivo e estado

Fixar a unidade de comparação, as métricas, a matriz experimental e o critério
de dominância antes de instrumentar o código, para que a rodada não possa
ajustar perfis retroativamente depois de observar cobertura.

O impasse tratado aqui é o veredito "go parcial" de
[0016](0016-certified-floating-dyadic.md): CFD-24 custa 41,7% menos que o FP32
exato restrito e perde disponibilidade em caos, CBD-32 custa ainda menos e
também abstém, e nenhuma das grandezas medidas até agora ordena as famílias.

## Arquivos modificados

- `docs/pareto-protocol.md` (novo).

## Decisões e hipóteses

1. A unidade de comparação passa a ser a decisão definitiva **correta**, não a
   decisão definitiva. Sem isso, formatos não certificados aparecem com
   cobertura de 100% por construção, o que torna a comparação vazia.
2. A métrica primária é `kappa = C_total / G`, constraints por decisão
   definitiva correta. Ela penaliza abstenção e erro pela mesma via.
3. Abstenção é modelada como custo recuperável, com
   `C_esc = C_p + (A_p/N) * (C_p' + L)`. `L` é o custo do vínculo entre provas
   em perfis diferentes e precisa ser medido em circuito; enquanto não for,
   `kappa_esc` é rotulada como modelo.
4. A fronteira é reportada em duas regiões, separadas por `omega = X/D`. Só a
   região com `omega = 0` pode sustentar alegação de segurança de decisão.
   Espera-se que a região não certificada tenha `kappa` menor em vários
   workloads; isso é o resultado a reportar, não um defeito.
5. `X` é nomeada "decisão definitiva incorreta", não "unsound". Para FP32 e
   ponto fixo, `X > 0` é comportamento esperado da aritmética finita. Para CBD e
   CFD, `X > 0` seria violação do teorema de inclusão e é defeito.
6. A junção entre custo e cobertura usa `profile_hash_hex`, SHA-256 sobre uma
   serialização canônica de perfil de experimento comum às duas famílias de
   binários. Os rótulos internos atuais não servem como chave: a CLI aceita
   `cbd` enquanto o JSON grava `cbd_local_taylor`.
7. A partição de regimes (estável, caótico, decisão única) é declarada agora,
   para impedir escolha post-hoc de recorte favorável.

## Comandos executados

```bash
cargo build --workspace --all-targets
```

Baseline compila sem erro antes de qualquer alteração.

## Testes, métricas e evidências

Nenhuma métrica nova. Este registro é normativo: ele define o que será medido
nos registros 0018 a 0022 e sob quais restrições.

Os números que motivam o protocolo já estão registrados e não são recalculados
aqui: 1.003 contra 1.721 constraints em CFD-24 sequencial versus FP32
(0016); 782 contra 1.721 em CBD-32 sequencial (0015); 494 decisões centrais
incorretas convertidas em `Indeterminate` no ECEF CBD-16 (0008).

## Tentativas malsucedidas

Foi considerado e descartado comparar por "custo por decisão definitiva" sem o
termo de correção. Formatos não certificados teriam `D = N` por construção, e a
métrica premiaria justamente a representação mais barata que erra mais. O termo
`X` é o que torna a comparação entre famílias legítima.

Também foi considerado usar o circuito Nova aumentado como grandeza primária de
custo. Foi descartado: o overhead estrutural constante domina as instâncias
medidas e comprime a diferença real entre representações de 2,43x para 8,4%,
como já registrado no relatório.

## Limitações conhecidas

1. A referência MPFR possui bound conservador. Oportunidades em que o próprio
   intervalo da referência cruza o limiar precisam ser excluídas da população e
   contabilizadas à parte; caso contrário o experimento atribuiria à
   representação um erro da referência.
2. `L` ainda não existe. Até o registro 0021, `kappa_esc` é modelo.
3. A matriz cobre quatro workloads e um backend. Nenhuma alegação sobre IEEE 754
   geral, outros ciclos de curvas ou outros esquemas de compromisso decorre
   dela.
4. O critério de dominância é definido sobre três eixos simultâneos. Ele produz
   frequentemente frentes incomparáveis, o que é a resposta correta quando não
   existe vencedor único.

## Próximo passo

Parametrizar `nova-circuit` e emitir `profile_hash_hex` dos dois lados, para que
a matriz da seção 5 do protocolo possa ser varrida. Registro 0018.

## Instrução de continuidade para outra IA

Leia `docs/pareto-protocol.md` antes de tocar em qualquer código de medição. Não
altere domínio, corredor, `radius_bits` ou limiares depois de observar
cobertura: o protocolo proíbe explicitamente, e um perfil barato com cobertura
ruim deve ser preservado como resultado negativo. Não use os rótulos de
representação como chave de junção; use `profile_hash_hex`.

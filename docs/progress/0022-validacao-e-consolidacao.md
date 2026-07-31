# 0022 — Validação: Berkeley SoftFloat e soundness exaustiva

Data: 2026-07-30. Estado: a dívida de validação de 0007 está paga. Berkeley
SoftFloat roda e passa; a soundness deixou de ser amostrada e passou a ser
enumerada.

## Objetivo e estado

Duas dívidas, ambas do tipo que um avaliador cobra na banca.

A primeira é de [0007](0007-referencia-fp32-restrita.md): a validação diferencial
contra Berkeley SoftFloat, o árbitro de fato do IEEE 754, nunca rodou. Sem ela, a
afirmação "o baseline FP32 está correto" apoiava-se num crate independente, o que
é melhor que nada e menos do que se pede.

A segunda é mais séria e não estava registrada como dívida. O projeto inteiro
testa as representações certificadas por amostragem — 10.000 entradas aqui,
200.000 ali. Amostragem não demonstra soundness, mostra ausência de contraexemplo
na amostra. A propriedade que o capítulo afirma é universal: *nenhuma* decisão
definitiva é incorreta. Uma afirmação universal precisa de argumento universal.

## Arquivos modificados

- `tools/softfloat-validation/src/lib.rs`: reescrito para validar
  `numeric-reference` em vez de `f32` nativo.
- `tools/softfloat-validation/Cargo.toml`: alvo restrito a x86_64 **Linux**;
  `numeric-reference` como dependência de teste.
- `crates/numeric-core/tests/exhaustive_soundness.rs` (novo).
- `scripts/run_softfloat_validation.sh` (novo).
- `.github/workflows/softfloat.yml` (novo).

## Decisões e hipóteses

**Por que Rosetta não resolve.** O build script de `softfloat-sys` decide as
regras por `cfg!(all(target_arch = "x86_64", target_os = "linux"))`, e num build
script `cfg!` avalia o **host**, não o alvo. Em Apple Silicon,
`--target x86_64-apple-darwin` compila o build script para aarch64-darwin e cai
no `unimplemented!()` de sempre. Não é questão de alvo, é questão de onde o build
script roda. Foi isso que 0007 não identificou, e por isso a tentativa 4 daquela
rodada parecia inexplicável.

O `cfg` do `Cargo.toml` foi corrigido de `target_arch = "x86_64"` para
`all(target_arch = "x86_64", target_os = "linux")`, que é a condição real.

**Contra Docker local, e mesmo assim com Docker local.** O plano recomendava
resolver por runner de CI ou VM, explicitamente não por Docker local. A
recomendação vinha da premissa de 0007 de que Docker em Apple Silicon roda
containers aarch64 — verdade por padrão, falsa com `--platform linux/amd64`, que
emula um host x86_64 Linux genuíno, indistinguível para o build script. As duas
saídas foram implementadas: `.github/workflows/softfloat.yml` para CI nativo, e
`scripts/run_softfloat_validation.sh` para reprodução local. Os números abaixo
vêm do segundo, porque este repositório ainda não tem remoto onde o primeiro
rodaria.

**O sujeito do teste mudou.** A versão de 0007 comparava `f32` nativo contra
Berkeley, o que valida o hardware da máquina e não este projeto. Agora o sujeito
é `restricted_logistic_step` e `restricted_stable_step`, que são o baseline FP32
efetivamente usado nos workloads. Os três intermediários são comparados, não só a
saída: um erro de arredondamento que se cancelasse no produto final passaria por
uma comparação só de saída.

**Como a enumeração pode ser exaustiva sobre um contínuo.** Há dois
quantificadores: "para todo enclausuramento representável" e "para todo real
dentro dele". O primeiro se resolve encolhendo o formato até caber — com 6 bits
fracionários são 65 mantissas e raios até 8, algumas centenas de casos.

O segundo não se resolve por enumeração, e é onde estaria a tentação de
discretizar fino e chamar de exaustivo. O que fecha é o mapa logístico ser
contínuo e unimodal em `[0,1]`, com único ponto crítico em `1/2`: a imagem de
`[a,b]` é exatamente `[min(f(a),f(b)), max(f(a), f(b), f(1/2) se 1/2 ∈ [a,b])]`.
Conferir esses extremos exatos é verificação completa do contínuo, não
amostragem densa dele.

## Comandos executados

```
scripts/run_softfloat_validation.sh
cargo test -p numeric-core --test exhaustive_soundness
cargo test -p numeric-core --test exhaustive_soundness -- --nocapture fixed_point
```

## Testes, métricas e evidências

**Berkeley SoftFloat**, host x86_64 Linux, 3 testes, 3 passaram:

| teste | amostras | resultado |
| --- | --- | --- |
| `logistic_reference_matches_berkeley_softfloat` | > 100.000 | bit a bit |
| `stable_reference_matches_berkeley_softfloat` | > 50.000 | bit a bit |
| `hard_rounding_cases_match_berkeley_softfloat` | 8 fixos | bit a bit |

Os 8 casos fixos são os que o arredondamento costuma quebrar: menor normal
positivo, maior valor abaixo de 1, `1,0` exato, `0,5` e `0,5 + 1 ulp`, `1/3` e
`2/3` arredondados, zero. Ficam fixos porque sorteio não garante visitá-los.

Compilação sob emulação: 7 min 54 s. Execução: 0,02 s.

**Enumeração exaustiva**, formato reduzido de 6 bits, 7 testes, 7 passaram:

| propriedade | escopo |
| --- | --- |
| enclausuramento contém a imagem exata, CBD | todos os enclausuramentos × 5 variantes de passo |
| enclausuramento contém a imagem exata, CFD | toda a grade × 2 topologias |
| nenhuma definitiva contradiz a imagem exata | todos os enclausuramentos |
| soundness sobrevive a 8 iterações | CBD e CFD, toda a grade |

Nenhum contraexemplo. Isto não é "não achamos erro em 200.000 amostras", é "não
existe erro no formato reduzido", que é uma afirmação de tipo diferente.

**O contraste, no mesmo formato reduzido.** O teste
`fixed_point_in_the_same_grid_produces_wrong_definite_decisions` roda ponto fixo
na mesma grade de 6 bits, 8 iterações do mapa logístico:

```
ponto fixo reduzido: 10 de 65 decisões incorretas
```

É o argumento do capítulo reduzido a um caso que cabe inteiro na cabeça e é
verificável por enumeração. Na mesma grade, com a mesma precisão, nas mesmas
iterações: o certificado nunca erra e às vezes se abstém; o ponto fixo sempre
responde e erra 15% das vezes. A diferença não é precisão, é saber quando não
sabe. O achado ECEF de 0008 e 0020 é este mesmo fenômeno em escala de aplicação.

## Tentativas malsucedidas

1. `cargo test --target x86_64-apple-darwin` com Rosetta: falha no mesmo
   `unimplemented!()` de 0007, pelo motivo explicado em Decisões. O alvo do teste
   não é o que o build script consulta.
2. Container amd64 sem `cmake`: `c99` não compila. `.cargo/config.toml` já traz
   `CMAKE_POLICY_VERSION_MINIMUM=3.5` desde 0007, mas o CMake precisa existir.
3. `use numeric_reference::fp32::...`: `fp32` é módulo privado, reexportado na
   raiz. Custou um ciclo de compilação de 8 minutos sob emulação para descobrir,
   e é a razão de o script existir em vez de o comando ser digitado à mão.
4. **Um parâmetro ECEF inventado.** A primeira versão de `sweep_decisions` em
   `frontier_eval.rs` usava limiar angular de `1e-3` rad, cerca de 6,4 km, com
   um comentário atribuindo o valor a 0008. Não era o de 0008, que usa 100 km
   sobre uma Terra de 6.371 km — 15,7 vezes mais largo. O erro inflava o
   resultado a favor do argumento: com o limiar apertado, o CBD-16 abstinha-se
   nos 2.000 casos e o ponto fixo errava 993, números mais dramáticos que os
   verdadeiros. Corrigido para os parâmetros de 0008, a cobertura CBD-16 passou a
   reproduzir aquela rodada (838 definitivas contra 837) e o ponto fixo a errar
   492 — que é o número que **casa** com as 494 discordâncias de centro medidas
   em 0008, e portanto vale bem mais como evidência do que o 993 inventado.

## Limitações conhecidas

1. A enumeração exaustiva vale para 6 bits fracionários. É demonstração de que a
   construção é sound, não prova para p=16 ou p=64 — para esses continua valendo
   o argumento analítico do enclausuramento mais os testes amostrados. O valor de
   6 bits foi escolhido por caber em meio segundo de CI; subir para 10 bits
   multiplica por 16 sem mudar a natureza da afirmação.
2. Berkeley SoftFloat cobre o baseline FP32 da **referência**, não o gadget R1CS
   de `nova-circuit/src/fp32.rs`. O gadget é testado contra a referência
   separadamente, então a cadeia fecha por transitividade, mas não há um teste
   único que vá do circuito a Berkeley.
3. O workflow de CI não foi executado: o diretório não é repositório git e não há
   remoto. O YAML reproduz passo a passo o que rodou sob emulação, mas está
   verificado por inspeção, não por execução.
4. A validação exaustiva cobre o mapa logístico. O afim é linear, e a mesma
   construção de imagem exata seria mais simples, mas não está escrita.

## Próximo passo

Os benchmarks Nova com repetições suficientes para intervalo de confiança, e a
redação do capítulo.

## Instrução de continuidade para outra IA

Para rodar Berkeley: `scripts/run_softfloat_validation.sh`, que precisa de Docker
com suporte a `linux/amd64`. A primeira execução leva cerca de oito minutos
compilando sob emulação; `CARGO_TARGET_DIR` aponta para `/tmp` dentro do
container, então nada disso contamina o `target/` local nem é reaproveitado entre
execuções — se for iterar, monte um volume para o alvo.

Se `exhaustive_soundness` começar a falhar, leia com atenção antes de mexer no
teste: um contraexemplo ali é um erro de soundness na representação certificada,
e derruba o capítulo, não o teste. `REDUCED_BITS` e `MAX_RADIUS` no topo do
arquivo controlam o tamanho da enumeração.

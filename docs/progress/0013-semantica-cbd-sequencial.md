# 0013 — Semântica CBD sequencial

**Data:** 2026-07-05  
**Estado:** concluído com hipótese falsificada

## Objetivo

Separar o efeito da representação CBD do efeito de quantizar a expressão
logística apenas uma vez.

## Arquivos modificados

- `crates/numeric-core/src/cbd.rs`
- `crates/numeric-core/tests/certified_dyadic.rs`
- `crates/numeric-core/tests/certified_dyadic.proptest-regressions`
- `docs/mathematical-specification.md`

## Decisões e hipóteses

`logistic_step_sequential_local_bound` quantiza `x(1-x)`, propaga o raio local,
multiplica o centro intermediário por `r` e quantiza novamente. O complemento
é exato na grade diádica. A dependência entre `x` e `1-x` é preservada pela
expansão `d(S-2N)-d²`.

## Comandos executados

```bash
cargo test -p numeric-core --test certified_dyadic
```

## Testes, métricas e evidências

- Inclusão racional em 12 passos composicionais.
- 256 casos baseados em propriedades para `x∈[0,1]` e `r∈[0,4]`.
- Perfis 16/32/48/64 contêm a referência na primeira etapa.
- Nenhum caso de inclusão incorreta foi encontrado.

## Tentativas malsucedidas

A hipótese “o intervalo fusionado nunca é mais largo” é falsa. O proptest
encontrou `x=21508/1000000`, `r=1/10`: centros arredondados diferentes e o
recorte em `[0,1]` invertem a largura visível. Aumentar artificialmente o raio
sequencial esconderia o resultado e foi rejeitado. O caso ficou preservado
como regressão.

## Limitações conhecidas

- Não existe ordem total ponto a ponto entre as larguras fusionada e
  sequencial.
- A comparação válida usa inclusão, cobertura e decisões incorretas.
- Horizontes longos continuam usando MPFR, não racional exato.

## Próximo passo

Aritmetizar os dois estágios de quantização e raio sem alterar a semântica.

## Continuidade para outra IA

Não restaure a alegação de ordenação de largura. Preserve o contraexemplo e
compare cobertura agregada.

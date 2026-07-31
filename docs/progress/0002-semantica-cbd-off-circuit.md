# 0002 — Semântica e referência racional CBD

**Data:** 2026-07-05  
**Estado:** concluído

## Objetivo

Formalizar a representação CBD e implementar sua referência racional antes de
aritmetizá-la.

## Arquivos modificados

- `crates/numeric-core/src/lib.rs`
- `crates/numeric-core/src/cbd.rs`
- `crates/numeric-core/tests/certified_dyadic.rs`
- `docs/mathematical-specification.md`
- `docs/thesis-execution-plan.md`

## Implementação

Foram adicionados:

- `NumericProfile`;
- `CertifiedDyadic`;
- `Decision3`;
- `OverflowPolicy::Reject`;
- quantização nearest-ties-to-even assinada;
- soma, subtração, multiplicação, multiplicação por fator público e FMA;
- mapa logístico fusionado e versão sequencial de ablação;
- escalonamento de perfil 16→32→48→64;
- decisão certificada por limiar.

O raio é o menor número inteiro de ULPs que contém o intervalo exato mantendo
o centro escolhido.

## Decisão semântica encontrada durante os testes

A denotação original usava apenas o intervalo simétrico:

```text
[(M-R)/S, (M+R)/S]
```

Um teste de propriedade encontrou o caso `left=right=1/65535`. Após
quantização, o produto tem centro zero e precisa de raio de uma ULP. O
intervalo simétrico bruto ultrapassa zero, embora todo valor matemático
admissível permaneça em `[0,1]`.

A solução adotada foi tornar o domínio público parte da denotação:

```text
[(M-R)/S, (M+R)/S] ∩ [L,U]
```

Isso evita deslocar o centro arredondado nos endpoints e explicita a semântica
de uma representação limitada.

## Comandos executados

```bash
cargo test -p numeric-core
```

## Testes e evidências

Passaram:

- 1 teste unitário interno CBD;
- 9 testes de integração CBD, incluindo 512 casos de propriedades;
- 9 testes preexistentes de limbs;
- doc-tests.

As propriedades cobrem inclusão na codificação, canonicalidade do raio,
operações, mapa logístico, nearest-even assinado, decisões trivaloradas e
escalonamento.

## Tentativas malsucedidas

- A primeira definição rejeitava qualquer intervalo simétrico que saísse do
  domínio. Isso falhou em operações próximas a zero.
- Considerou-se deslocar o centro para dentro do domínio, mas isso alteraria o
  resultado do arredondamento e confundiria erro numérico com tratamento de
  endpoint.

## Limitações conhecidas

- O raio usa a mesma escala da mantissa e pode ser conservador em até uma ULP.
- Não há compromisso de entrada implementado.
- `logistic_step_fused` é específico para domínio contido em `[0,1]` e
  `r>=0`.
- Ainda não existe circuito CBD.

## Próximo passo

Definir testemunha inteira para uma step CBD do mapa logístico e implementar
um circuito R1CS que verifique centro, raio canônico e decisão.

## Continuidade para outra IA

Preserve a denotação intersectada com `[L,U]`. Antes de otimizar constraints,
faça o circuito reproduzir exatamente os valores retornados por
`CertifiedDyadic::logistic_step_fused`.

# Metodologia

Ordem de execução:

1. revisar bibliotecas;
2. fixar a especificação;
3. implementar referência racional;
4. implementar aritmética fora do circuito;
5. validar com propriedades;
6. executar o mapa logístico;
7. implementar a step circuit;
8. testar aceitação e rejeição;
9. integrar Nova;
10. medir constraints e desempenho;
11. comparar formatos;
12. redigir conclusões.

FP64 é baseline, nunca referência matemática. Custos IEEE 754 são separados em
três níveis: modelo local de subconjunto, custo publicado externo e gadget
medido homogêneo. O projeto já gera o primeiro nível em
`results/fp32_constraint_model.csv`; ele não substitui um gadget FP32 bit-exato
em Nova. Constraints, multiplicações não lineares, range checks, variáveis,
estado, prova e tempos são reportados separadamente.

## Semântica numérica de primitivas

Para funções não polinomiais, `NumericRepresentation` não é usado como sinônimo
de semântica. Cada ponto experimental declara:

```text
NumericSemantics = representation
                 + operation semantics
                 + approximation strategy
                 + error contract
```

O arithmetisation gap é observado por estados contrafactuais separados:
`f(x)`, `Q(f(Q(x)))`, `A(Q(x))`, `AQ(Q(x))` e a região aceita pela relação.
Esses componentes não são somados por padrão. Uma composição só recebe o rótulo
`certified` quando a regra e todos os bounds necessários têm evidência formal,
analítica ou exaustiva sobre domínio finito.

Máximos em malha, inclusive os obtidos com MPFR, permanecem `ObservedGrid`.
Uma decisão só é `CertifiedStable` quando o enclosure total fica integralmente
de um lado da fronteira. Caso contrário ela é `Ambiguous`, `ObservedSame` ou
`ObservedFlip`.

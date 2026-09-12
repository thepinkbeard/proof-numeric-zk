# 0025 - Semântica numérica e funções não polinomiais

## Objetivo e estado

Entrega off-circuit implementada para separar representação, operações,
aproximação e contrato de erro, incorporando de forma rastreável conceitos de
Sridhar et al. sem substituir os workloads existentes.

## Arquivos principais

- `numeric-core`: contrato público e fixed-point assinado;
- `numeric-reference`: `exp`/`sigmoid` MPFR e regras Gauss-Legendre;
- `numeric-approximations`: Taylor, Padé e relação GL;
- `function-eval`: matriz, resultados, decisões, manifests e timing;
- `profiles/`, `experiments/` e scripts de execução/figuras;
- documentação de semântica e reprodutibilidade.

## Decisões

1. `NumericContractProfile` não substitui tipos ou hashes legados.
2. `floor` do paper e RNE do artefato são perfis distintos.
3. Somente evidência formal, analítica ou exaustiva pode certificar.
4. Gauss-Legendre é uma relação implícita; o output honesto é canônico.
5. Não há estimativa de constraints nesta entrega off-circuit.
6. A faixa do artefato é separada da matriz científica.

## Comandos de verificação

```bash
cargo test --workspace
cargo run --release -p experiment-runner --bin function-eval -- \
  --matrix experiments/non_polynomial_v1.json \
  --output-dir /tmp/non-polynomial-smoke \
  --grid-points 3 --random-samples 1 \
  --timing-repetitions 1 --max-configurations 11
python3 scripts/plot_non_polynomial.py \
  --input-dir /tmp/non-polynomial-smoke \
  --output-dir /tmp/non-polynomial-smoke-figures
```

## Evidências

- a suíte completa passou com 160 testes (134 preservados e 26 adicionais);
- vetores Q16 de raízes/pesos GL e coeficientes Padé coincidem com o artefato;
- MPFR-512 permanece dentro do enclosure MPFR-256 nos testes;
- largura GL cresce com a ordem enquanto depth da relação permanece quatro;
- resultados observados sem bound total nunca viram decisões certificadas;
- os sete SVGs foram gerados e inspecionados visualmente.

## Tentativas malsucedidas

A primeira versão escolhia, para GL, o vizinho fixed-point com menor residual.
Isso alterava `exp(0)=1` para `1-ulp` e misturava output honesto com liberdade
adversarial. O núcleo passou a quantizar canonicamente a solução e registrar o
slack separadamente.

## Limitações e próximo passo

Ainda faltam circuitos Nova/R1CS para as relações, bounds globais para todos os
métodos, as demais funções e adaptação de precisão/ordem. O próximo passo deve
selecionar uma relação GL de sigmoid, portar seus range checks para R1CS e medir
constraints reais sem importar custos do backend Noir.

## Continuidade

Leia `docs/numeric-semantics-non-polynomial.md`, execute a suíte e um smoke run,
e use `numeric_profile_hash_hex` para qualquer junção de resultados desta nova
família. Não altere os perfis públicos v1 sem incrementar o schema.

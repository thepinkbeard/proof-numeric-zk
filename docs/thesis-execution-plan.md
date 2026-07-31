# Execução da Representação Diádica Certificada

## Objetivo

Projetar, implementar e avaliar a **Certified Bounded Dyadic (CBD)**: uma
representação numérica orientada a provas de conhecimento zero que certifica
decisões sobre valores reais limitados sem exigir equivalência bit a bit com
IEEE 754.

O critério principal é segurança da decisão. Quando o erro certificado não
permite concluir um predicado, o resultado deve ser `Indeterminate` e a mesma
entrada pode ser submetida novamente com um perfil de maior precisão.

## Documentação contínua

- `docs/progress/README.md` é o índice cronológico e ponto de retomada.
- Cada avanço gera um arquivo novo `docs/progress/NNNN-titulo-curto.md`.
- Registros concluídos são imutáveis; correções são registradas em um novo
  arquivo.
- Nenhum avanço é concluído sem registrar objetivo, estado, arquivos,
  decisões, comandos, testes, evidências, falhas, limitações e próximo passo.
- Falhas relevantes e caminhos abandonados também recebem registro próprio.
- Logs extensos e dados brutos permanecem em `data/` ou `results/` e são
  apenas referenciados nos registros.

## Representação CBD

Um perfil público fixa versão, domínio, escala binária, largura, modo de
arredondamento, política de overflow, limiares e próximo perfil.

Um valor CBD é formado por uma mantissa inteira assinada `M` e um raio inteiro
não negativo `R`, ambos interpretados na mesma escala `2^-F`:

\[
\llbracket M,R,F\rrbracket =
[(M-R)2^{-F},(M+R)2^{-F}]\cap[L,U],
\]

onde `[L,U]` é o domínio público do perfil. A interseção permite representar
erros simétricos junto aos endpoints sem afirmar que valores fora do domínio
são admissíveis.
As operações `add`, `sub`, `mul` e `fma` usam acumuladores inteiros largos,
round-to-nearest-ties-to-even e propagação conservadora de raio. A
representação rejeita overflow e não admite wrap modular.

Predicados retornam:

- `True`, quando todo o intervalo satisfaz o predicado;
- `False`, quando todo o intervalo rejeita o predicado;
- `Indeterminate`, quando o intervalo cruza a fronteira.

Os perfis iniciais usam 16, 32, 48 e 64 bits fracionários. Uma nova prova em
maior precisão deve permanecer ligada ao mesmo compromisso de entrada.

## Ordem de execução

1. Registrar plano, inventário e resultados existentes.
2. Formalizar a semântica CBD e seus bounds.
3. Implementar a referência racional off-circuit.
4. Adicionar testes unitários e baseados em propriedades.
5. Implementar operações fusionadas e ablações.
6. Criar a step circuit CBD em Nova/R1CS e testes adversariais.
7. Implementar FP32 exato restrito aos domínios avaliados no mesmo backend.
8. Validar FP32 contra Berkeley SoftFloat/TestFloat.
9. Integrar CBD ao IVC e à compressão Nova.
10. Avaliar mapa logístico, logistic regression e proximidade ECEF.
11. Consolidar fronteiras de Pareto, limitações e conclusões.

## Estado da execução em 2026-07-05

- Etapas 1–7 e 9–10 concluídas no escopo atual.
- Etapa 8 parcialmente concluída: 10.000 vetores coincidem com uma
  implementação software independente, mas Berkeley SoftFloat/TestFloat exige
  um host x86_64 real.
- O marco FP32 homogêneo mediu 1.721 constraints por etapa contra 709 no
  CBD-32. A decisão go/no-go é continuar o CBD.
- A ablação sequencial mediu 782 constraints em CBD-32 e 1.402 em CBD-64,
  ainda abaixo do FP32. Logo, a vantagem bruta observada não depende apenas da
  quantização fusionada.
- Próximo marco: vincular o escalonamento 16→32→48→64 ao mesmo compromisso de
  entrada e implementar o workload ECEF em circuito.

## Estado da execução em 2026-07-30

O "go parcial" de 0016 está resolvido. A unidade de comparação deixou de ser
constraints e passou a ser custo por decisão correta, e a fronteira que resulta
está em [`chapter-certified-frontier.md`](chapter-certified-frontier.md),
registros [0017](progress/0017-protocolo-pareto.md) a
[0023](progress/0023-medicao-estatistica.md).

- **Fronteira.** 694 configurações de custo juntas a 734 registros de cobertura
  por `profile_hash_hex`, 94,6% de junção, quatro workloads e quatro famílias.
  A junção é por hash canônico de perfil, não por convenção de nome.
- **Veredito por regime.** Em regimes contrativos o prêmio de certificação é de
  2,2 a 2,6×. Em caos com limiar `1/2` ou `3/4` nenhuma configuração não
  certificada é sound, e o prêmio não é definível por falta de comparação.
- **Segurança.** Em ECEF, ponto fixo de 16 bits erra 492 de 2.000 decisões;
  CBD-16, na mesma grade, decide 838 sem errar nenhuma e abstém-se no resto.
  Casa com as 494 discordâncias de valor central medidas em 0008.
- **Etapa 8 concluída.** Berkeley SoftFloat roda e passa em host x86_64 Linux.
  A causa do bloqueio era o `cfg!` do build script avaliar o host, não o alvo.
- **Soundness.** Enumeração exaustiva em formato de 6 bits, sem contraexemplo;
  ponto fixo na mesma grade erra 10 de 65.
- **Escalonamento.** O vínculo existe em circuito e custa 1.281 constraints,
  cinco vezes um passo de CBD-16. O termo de escalonamento deixou de ser
  modelado.
- **Tempos.** Desenho pareado com 30 rodadas intercaladas substitui as 3
  repetições que não sustentavam ordenação.
- Próximo marco: circuitos de produto interno para Adult e ECEF, que hoje têm
  cobertura nas quatro famílias mas nenhum custo medido.

## Baselines e métricas

Baselines: Q16.16, Q32.32, limbs 32/48/64, compensado, FP32/FP64 software e
FP32 R1CS homogêneo.

Métricas:

- inclusão da referência exata;
- horizonte até tolerâncias;
- cobertura de decisões definitivas;
- decisões incorretamente certificadas;
- taxa de escalonamento;
- constraints, range checks e elementos de estado;
- circuito Nova aumentado;
- tempos de fold, compressão e verificação;
- tamanho da prova, memória e custo por decisão definitiva.

Resultados de trabalhos externos são contexto, não benchmarks homogêneos:

- Garg et al., *Succinct Zero Knowledge for Floating Point Computations*;
- Ernstberger et al., *Zero-Knowledge Location Privacy via Accurate
  Floating-Point SNARKs*;
- Riasi et al., *Zero-Knowledge AI Inference with High Precision*.

## Critérios de sucesso

- A referência exata permanece dentro do intervalo CBD.
- Circuito e execução off-circuit produzem estados idênticos.
- Witness adulterado, raio insuficiente, perfil incorreto, arredondamento
  falso e wrap modular são rejeitados.
- Toda decisão definitiva concorda com a referência racional/MPFR.
- Alegações de menor custo exigem comparação homogênea e dominância na
  fronteira custo-cobertura.
- O primeiro marco go/no-go é uma etapa do mapa logístico com CBD e FP32
  restrito no mesmo backend.

## Premissas

- “Constantes” no texto original é interpretado como constraints R1CS.
- Backend principal: Nova 0.73, Pallas/Vesta e IPA.
- Fidelidade significa decisão certificada, não igualdade bit a bit com FP32.
- NaN, infinito, subnormal e expoente dinâmico ficam fora da CBD v1.
- Resultados negativos fazem parte do histórico experimental.

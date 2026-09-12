# CDAD: auditoria e contrato composicional, versão 1

Este marco implementa uma base **off-circuit**, não uma demonstração ZK completa
nem um resultado de eficiência. Experimentos históricos e hashes de seus perfis
não são reinterpretados. O contrato abaixo é uma **extensão nossa**, inspirada
na relação implícita de sigmoid de Sridhar, e não uma reprodução de seu circuito.

## Diagnóstico e mudanças

| Ponto auditado | Tratamento nesta entrega | Limite remanescente |
|---|---|---|
| GL histórico usava 16 ULP e conversões por derivada sem contrato total | `function-eval` esquema 2 classifica esse slack como hipótese e não fornece output bound certificado GL | Nova relação abaixo deve ser traduzida e validada em circuito |
| Referência de decisão usava o valor MPFR central | Decisão por enclosure dirigido; refinamento até 4096 bits e referência inconclusiva explícita | Precisão finita não resolve todo limiar |
| Pré-avaliações adaptativas não eram custo completo | Novo seletor registra todas as avaliações, inclusive `preview-all` | Sem custo ZK, nenhuma conclusão de eficiência |
| Compromisso vetorial incompleto | Entradas canônicas comuns são argumentos obrigatórios do checker | Autenticação criptográfica vetorial ainda pendente |
| Profundidade teórica confundível com medição | Relação de verificação exportada como `null` no runner v2 | Contagens do simulador não são constraints |
| Configuração efetiva e fontes não rastreadas | Manifesto v2 inclui overrides e hashes de fontes não commitadas | Arquivar fontes junto ao resultado final |

O novo `CheckedEnclosure` tem campos privados e não aceita desserialização.
Certificados serializados são tratados como não confiáveis. O checker refaz a
aritmética racional; uma etiqueta `Analytical` não autoriza uma decisão. O chamador
ainda é responsável por fornecer a entrada e o contrato autorizados. O tipo não
substitui uma prova de execução nem impede um chamador de fornecer entradas falsas.

## Relação verificada

Considere entrada canônica x em [-1,1], quantização Q, saída diádica y em [1/4,3/4],
nós r_i em [-1,1], pesos positivos w_i, todos constantes racionais quantizadas.
Seja a=2y-1 e D_i=1+a r_i. Então D_i >= 1/2.

Os witnesses diádicos h_i devem satisfazer exatamente:

```
|D_i h_i - a| <= delta_term
|sum_i w_i h_i - Q(x)| <= delta_sum
```

Os produtos e a soma ponderada dessas desigualdades são racionais exatos, sem
arredondamento intermediário implícito. O gerador escolhe `delta_term=2 ULP` e
`delta_sum=16 ULP`; esses valores definem **nossa relação**, não um teorema
atribuído a Sridhar. O checker também aceita outros slacks não negativos e
recalcula a garantia. Um witness honesto pode ser rejeitado; isso afeta
completude/disponibilidade, não autoriza relaxamento silencioso.

## Bound independente da precisão do gerador de nós

Para b=1/2 e grau d, calcule exatamente:

```
mu_k = 2/(k+1) se k par, e 0 se k ímpar
E_quad = sum_{k=0}^d b^(k+1) |mu_k - sum_i w_i r_i^k|
         + (2 + sum_i w_i) b^(d+2)/(1-b)
```

Justificativa: expandir a/(1+a t) em série geométrica, integrar termo a termo em
[-1,1] (convergência uniforme pois |a t|<=1/2) e limitar o restante pela série
geométrica. Os defeitos de momentos usam os **nós e pesos quantizados reais**,
incluindo a inexatidão de sua geração. Não é necessário confiar que os números
fornecidos sejam raízes exatas de Legendre.

A integral é `log(y/(1-y))`. Das relações aceitas, para **qualquer** h_i:

```
|h_i - a/D_i| <= 2 delta_term
|logit(y) - x| <= E_quad + 2 sum_i w_i delta_term
                         + delta_sum + |Q(x)-x|
```

Como sigmoid tem derivada globalmente limitada por 1/4:

```
|y - sigmoid(x)| <= (E_quad + 2 sum_i w_i delta_term
                    + delta_sum + |Q(x)-x|)/4
```

O certificado deve declarar um raio diádico **pelo menos** igual ao bound
recalculado, arredondado para fora. Domínios, grade, largura, hash, entrada,
denominadores e todas as desigualdades são verificados. O enclosure é
intersectado com [0,1]. Essa é uma justificativa analítica implementada com
racionais exatos, ainda não uma prova mecanizada em Coq/Lean nem uma prova R1CS.

## Composição e decisão

Soma, subtração, escala com sinal e quadrado propagam enclosures por regras
intervalares. Requantização usa centro conforme o formato e raio arredondado
para fora; overflow dos componentes é rejeitado. Para a média de quatro
sigmoids, somam-se os quatro intervalos e divide-se por quatro. Não se presume
independência estatística dos erros.

No caso geral, `e_next <= L e + eta` exige um L válido em todo domínio alcançável.
O teste de perturbação local seguida de escala 1024 demonstra que erro pequeno
local pode atravessar a fronteira. É um controle inspirado pela não composição,
não uma reprodução do exemplo de rede neural de Zamir.

Para o predicado y>=tau: `True` se lower>=tau; `False` se upper<tau; caso
contrário `Indeterminate`. Não se converte ausência de flip observado em prova.

ECEF tem ainda um baseline exato de soma de três diferenças quadráticas, sem
raiz ou reescala dos produtos. Sobre uma grade canônica comum isso equivale a
aritmética inteira com denominador comum. O exemplo executável usa coordenadas
normalizadas sintéticas próximas à fronteira, não substitui o dataset ECEF.

## Seleção e privacidade

`fixed`, `sequential`, `margin-directed` e `preview-all` usam a escada
16/32/48/64. A previsão por margem pode falhar (por exemplo, erro de quadratura
não cai só por aumentar F); somente o enclosure validado decide. `preview-all`
sempre computa os quatro candidatos e seleciona o primeiro definitivo, ou 64.
Tempo total de seleção inclui callbacks e controle, sem ocultar candidatos.

A precisão é pública e pode revelar informação sobre margem/dificuldade da
entrada. O vínculo atual é igualdade de argumentos e hash de contrato, **não**
um compromisso criptográfico. Tradução futura: comprometer o vetor canônico,
demonstrar cada quantização a partir dele, usar produtos em escala ampliada,
range checks para impedir wrap, e validar a decisão e os bounds públicos.

## Reproduzir o marco

```
cargo test --workspace --locked
cargo run --locked -p experiment-runner --bin contract-audit -- \
  --output-dir results/cdad-contract-audit-NOVO
```

O diretório pai deve existir; o diretório final deve ser novo. São exportados
contratos, certificados, enclosures, todas as avaliações e manifesto. Timings
dessa execução pequena incluem overhead do auditor e não suportam comparação
de desempenho. Constraints/provas/verificação permanecem `null`.

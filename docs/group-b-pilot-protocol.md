# Grupo B: protocolo do primeiro piloto off-circuit

Configuração congelada antes de avaliar: `experiments/qualification_group_b_pilot_v1.json`.
Este piloto valida a instrumentação das figuras 5–7 de 0028. Não substitui a
campanha de dez seeds e mil oportunidades por seed do protocolo competitivo.
Não permite uma conclusão estatística de eficiência nem reprodução do ZIP/NFGen.

## Dados e referência

- Oito vetores de quatro coordenadas independentes. Para cada seed, SplitMix64
  reinicia o estado, incrementa por `0x9e3779b97f4a7c15` e aplica os dois
  multiplicadores padrão explícitos no código. Cada saída inteira `j` produz
  `z=j/2^63-1`, em `[-1,1)`. Os valores são racionais, não conversões f64.
- Malha escalar com 17 pontos `-1+2k/16`, usada somente para comparar métodos.
- Cinco vetores adversariais separados: todos -1, todos +1, pares
  `[-3/4,-1/4,1/4,3/4]`, e os mesmos pares com a primeira coordenada deslocada
  por `+2^-24` ou `-2^-24`. Nenhum entra nas taxas do conjunto aleatório.
- A referência da média soma exatamente enclosures MPFR dirigidos de 512 bits.
  Se algum limiar não for separado, duplica a precisão até 16384 bits. Não usa
  o valor central para decidir. Igualdades simétricas podem permanecer
  inconclusivas; não são convertidas em acertos. Erro observado é um intervalo
  contra a referência, não uma distância f64 alegadamente exata.

## Relações, garantias e métodos

GL reutiliza o checker racional de `docs/cdad-compositional-contract.md`, com
nós/pesos quantizados, slack por termo `2 ULP`, slack de soma `16 ULP` e bound
de momentos com cauda geométrica. Essas escolhas e sua conversão à decisão
são **extensão nossa**, não reprodução de um circuito Sridhar.

A média é uma soma racional exata dividida por quatro. Não há arredondamento
adicional nessa composição: o resultado pode ter F+2 bits fracionários. Um
futuro circuito deverá implementar essa semântica ou cobrar a requantização.
Taylor/Padé reutilizam Horner fixed-point do repositório; nesta entrega seus
erros de sigmoid são **observados**, sem cobertura certificada. As contagens
Horner são numéricas; profundidades de circuito e constraints ficam ausentes.
Os métodos não são representações diferentes: compartilham formato, domínio
e arredondamento. Baselines otimizados por partes/lookup permanecem pendentes;
não se conclui superioridade de GL a partir deste subconjunto de comparadores.

## Witnesses aceitos e composição controlada (figura 5 inicial)

Antes de ver resultados, fixam-se `F=16`, ordem 4, entradas `{-3/4,0,3/4}` e
deslocamentos inteiros da saída honesta em `[-32,32] ULP`. Cada candidato
recalcula os termos honestamente para a nova saída, mas só é considerado
admissível se passar **todas** as condições do checker. Armazenam-se os
witnesses aceitos, e rejeições contam como rejeições, não como erros numéricos.
A busca restringe os termos: é um pior caso encontrado, não máximo universal.

Depois da primitiva, aplica-se exatamente `v <- 2v`, por k em `{0,1,2,4,8}`.
Assim, o desvio e o bound propagam-se por `2^k`, sem novo erro local. Este
controle de sensibilidade não é uma rede ZIP, nem composição repetida de
sigmoids. Não há domínio sigmoid a impor às saídas dessas etapas lineares.
O bound mostrado é sobre a distância à função de referência para **cada**
witness aceito; não é o diâmetro do conjunto de saídas aceitas.

## Precisão, ordem e política (figura 7 inicial)

Todos os vetores usam GL `{2,4,8,16}` e F `{16,32,48,64}`, com decisões nos
limiares `{2/5,1/2,3/5}`. Certificação exige enclosure validado pelo checker.
Contradição com referência conclusiva invalida a execução.

As quatro políticas existentes são reaplicadas aos enclosures armazenados,
sem acesso à referência. Todas as precisões visitadas são exportadas. Trata-se
de **replay lógico**, não medição de seleção ou prova; nenhum tempo de lookup
em cache será apresentado como vantagem adaptativa. Precisões fixas são
mostradas individualmente. Previews, witness, setup e prova precisam de uma
campanha própria para comparar custo completo.

## Reprodutibilidade e critérios

Diretórios novos, matriz e entradas exportadas antes da avaliação, hashes dos
artefatos, fontes, Cargo.lock e diff, commit/toolchain/ambiente registrados.
Arquivo de conclusão só existe após terminar todas as validações. Timings
diagnósticos ficam separados e não integram a comparação byte a byte.
Verificação em novo processo reconstrói entradas/matriz, revalida contratos e
witnesses, recompõe as médias e recalcula as decisões. Não é verificação ZK.

Figuras são geradas dos registros brutos, com conjuntos separados e denominador
explícito. Nenhum máximo de malha vira certificado. A garantia vem do checker
e de seu argumento analítico, não da ausência de falhas observadas no piloto.

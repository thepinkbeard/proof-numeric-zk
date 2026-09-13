# 0028 — Comparações e gráficos para fundamentar a proposta de tese

Data: 2026-09-13. Estado: pesquisa consolidada e documentação concluída;
comparações e figuras novas propostas, não executadas nesta etapa.

Base inspecionada: commit `eb396c9f1436693932d2cfb1cbc393424e0b82ff`.
Continuação aditiva de [0027](0027-qualification.md), preservando os registros,
perfis, experimentos e resultados históricos.

## 1. Objetivo e estado

Organizar comparações com a literatura e especificar quais gráficos podem
fundamentar a proposta de tese, explicando por que e como produzi-los. Esta
entrega é exclusivamente documental: não implementa baselines, não treina
modelos, não executa experimentos, não gera provas ou figuras e não corrige
código experimental.

A recomendação científica organiza a evidência em duas frentes:

- **Sistemas dinâmicos:** demonstrar por que uma execução numericamente válida
  pode não justificar a decisão matemática pretendida.
- **Inferência com sigmoid:** demonstrar quando contratos de erro composicionais
  permitem preservar uma decisão útil, apesar de aproximações numéricas.

O caos evidencia a necessidade e os limites da proposta. A inferência oferece
uma aplicação em que a certificação pode ser útil antes de exigir precisão
muito alta. A prioridade é composição controlada de quatro sigmoids, seguida
de um modelo pequeno sobre MNIST. ECEF permanece preservado, fora do caminho
prioritário desta rodada.

A pesquisa considera fontes primárias disponíveis até **13 de setembro de
2026**. É uma revisão direcionada, não uma demonstração exaustiva de
originalidade. A gravação deste registro consolida a pesquisa já apresentada;
não constitui uma nova reprodução dos artefatos citados.

## 2. Arquivos modificados

- Novo registro: `docs/progress/0028-comparacoes-literatura-e-graficos.md`.
- Índice atualizado: [README.md](README.md).

Nenhuma API, tipo público, dependência, circuito, script, configuração de
experimento ou resultado foi alterado. As alterações preexistentes em arquivos
`.DS_Store` não fazem parte desta entrega e foram preservadas.

## 3. Decisões e hipóteses

### 3.1. Hipótese e posicionamento

> Dentro de domínios explicitamente delimitados, contratos numéricos
> composicionais permitem certificar decisões preservadas por qualquer witness
> aceito pela relação implementada. A adaptação pode aumentar a cobertura; sua
> vantagem de custo precisa ser demonstrada separadamente.

Essa garantia depende da validade do contrato, da correspondência entre o
contrato e a relação implementada e das hipóteses criptográficas aplicáveis.
Uma implementação apenas off-circuit não estabelece, por si só, essa
correspondência com um circuito ZK.

Não tentaremos demonstrar que outras representações "produzem provas falsas".
A comparação será entre **a relação que a prova certifica** e **a propriedade
que a aplicação necessita**:

- **L1 — Cryptographic Soundness:** a prova certifica a relação implementada.
- **L2 — Semantic Fidelity:** representação, operações e contrato correspondem
  à semântica numérica declarada.
- **L3 — Application-Level Correctness:** o contrato sustenta a decisão
  pretendida pela aplicação.

Representação, ordem das operações, arredondamento, estratégia de aproximação e
contrato de erro serão fatores separados. Uma divergência FP32 frente à
trajetória matemática não será chamada de descumprimento de IEEE nem de falha
criptográfica.

### 3.2. Trabalhos prioritários e o que comparar

| Trabalho | Relevância para a tese | Comparação recomendada |
|---|---|---|
| **Sridhar et al. — Efficiently Provable Approximations for Non-Polynomial Functions. USENIX Security 2026.** | Apresenta aproximações por Gauss–Legendre com quantização e profundidade multiplicativa limitada. A implementação usa Noir/Barretenberg. | Na mesma representação, comparar Taylor, Padé e GL: erro observado, bound, largura das saídas admissíveis, ordem e custo. Investigar depois a composição dessas aproximações. [Artigo](https://www.usenix.org/conference/usenixsecurity26/presentation/sridhar). |
| **Riasi et al. — Zero-Knowledge AI Inference with High Precision, ZIP. CCS 2025.** | Combina operações lineares IEEE com verificação aproximada de ativações. A relação local limita a distância entre uma saída e um polinômio. | Comparar precisão da execução honesta e liberdade permitida pela relação; verificar se o contrato composto separa a fronteira de classificação. [Paper dos autores](https://thanghoang.github.io/publication/25_ccs_zip/25_ccs_zip.pdf). |
| **Bitan et al. — Sum-Check Protocol for Approximate Computations. EUROCRYPT 2026.** | Trata aproximação dentro do protocolo; sua análise relaciona tolerância numérica, erro da alegação e probabilidade de rejeição. | Comparar garantias e hipóteses, não tempo diretamente com Nova. Distinguir tolerância numérica de erro de soundness criptográfica. [Publicação](https://weizmann.elsevierpure.com/en/publications/sum-check-protocol-forapproximate-computations/). |
| **Chen et al. — ZKML: An Optimizing System for ML Inference in Zero-Knowledge Proofs. EuroSys 2024.** | Usa ponto fixo, escalas, lookups para não linearidades e otimização de circuitos Halo2. | Comparar políticas de quantização e preservação da classificação no mesmo modelo. Incluir lookup como alternativa competitiva às aproximações polinomiais. [Paper](https://ddkang.github.io/papers/2024/zkml-eurosys.pdf). |
| **Garg et al. — Succinct Zero Knowledge for Floating Point Computations. CCS 2022.** | Antecedente relevante de computação verificável com erro relativo limitado. | Comparar propagação de contratos relativos com contratos centro–raio. Uma implementação local desse modelo não reproduz automaticamente o protocolo completo. [Paper](https://par.nsf.gov/servlets/purl/10408517). |
| **Fan et al. — NFGen: Automatic Non-linear Function Evaluation Code Generator for General-purpose MPC Platforms. CCS 2022.** | Gera aproximações polinomiais por partes para ponto fixo. É um trabalho de **MPC, não ZK**. | Usar como baseline de aproximação, evitando comparar GL somente com Taylor ingênuo. Não comparar tempos de MPC com tempos de prova Nova. [Paper e identificação oficial](https://doi.org/10.1145/3548606.3560565). |
| **Ernstberger et al. — Zero-Knowledge Location Privacy via Accurate Floating-Point SNARKs. IEEE S&P 2025.** | Liga aritmética floating-point e proximidade geográfica. | Manter como referência para uma futura comparação ECEF, sem incluí-la no caminho prioritário de inferência. [Publicação](https://discovery.ucl.ac.uk/id/eprint/10211442/). |
| **Zamir — A Note on Non-Composability of Layerwise Approximate Verification for Neural Inference. Preprint, fevereiro de 2026.** | Mostra que tolerâncias locais, sem hipóteses adicionais, não justificam automaticamente uma conclusão global sobre a inferência. | Construir testes de composição com perturbações adversariais permitidas. Não é evidência de que todos os sistemas aproximados estejam incorretos. Publicação em conferência não confirmada nesta revisão. [Preprint](https://arxiv.org/abs/2602.15756). |

### 3.3. Antecedentes e limites de originalidade

- **Precisão adaptativa orientada a predicados não é nova:** Shewchuk já a
  utiliza em predicados geométricos robustos. [Trabalho e implementação do
  autor](https://www.cs.cmu.edu/~quake/robust.html).
- **Representação centro–raio e precisão arbitrária certificada não são novas:**
  Arb é um antecedente direto. [Artigo de
  Johansson](https://arxiv.org/abs/1611.02831).
- **Composição rigorosa de análise numérica e correção de implementação tem
  antecedentes:** VCFloat2, de Appel e Kellison, CPP 2024, oferece uma referência
  de rigor. [Paper](https://www.cs.princeton.edu/~appel/papers/vcfloat2.pdf).
- **Abstenção e curvas risco–cobertura possuem literatura consolidada:** aproveitar
  essa metodologia distinguindo garantias estatísticas de certificados numéricos
  determinísticos. [Geifman e El-Yaniv, NeurIPS
  2017](https://arxiv.org/abs/1705.08500).

A contribuição candidata não será "inventamos precisão adaptativa", mas
**conectar contrato composicional, relação verificável, decisão e custo da
adaptação**. Numeric profiles, CDAD, seleção por margem e a composição com Nova
são extensões deste projeto; não serão atribuídos a Sridhar.

### 3.4. Grupo A — Demonstrar o problema e o ganho semântico

#### Figura 1 — Trajetórias diferentes, decisões diferentes, provas válidas

**Como:** dois painéis para o mapa logístico, um estável e outro caótico. Eixo
horizontal: iterações. Eixo vertical: estado. Mostrar referência dirigida,
fixed-point, FP32 restrito e centros CBD; acrescentar limiar e marcações das
decisões divergentes.

**Comparar:** mesmos parâmetros e entrada canônica, com ordem de operações e
arredondamento identificados. Usar o caso histórico predefinido como ilustração,
sem escolher retrospectivamente a trajetória mais favorável.

**Por que:** comunica a distinção entre executar corretamente uma representação
e preservar a dinâmica matemática pretendida.

**Limite:** a matriz longa é numérica/off-circuit. Só marcar prova verificada
nos casos com artefato criptográfico correspondente; as provas curtas não provam
automaticamente os horizontes longos. Divergência de trajetória não invalida
automaticamente propriedades estatísticas do sistema. Também não equivale a
uma análise de shadowing, que pode envolver uma trajetória exata de outra
condição inicial. [Sauer, Grebogi e Yorke, Physical Review Letters
1997](https://journals.aps.org/prl/abstract/10.1103/PhysRevLett.79.59).

**Disponibilidade:** grande parte dos dados já existe.

#### Figura 2 — Responder corretamente, responder incorretamente ou abster-se

**Como:** barras empilhadas por horizonte e representação, separando concordância
observada, divergência, abstenção e referência inconclusiva. Identificar
visualmente quais respostas possuem contrato certificado. Explicitar os
denominadores e não contar uma oportunidade duas vezes.

**Comparar:** fixed-point truncamento/RNE, FP32, CBD, CFD e políticas CDAD.

**Por que:** torna visível o ganho principal: não transformar uma aproximação
insuficiente em uma resposta definitiva injustificada.

**Limite:** concordância observada não será chamada de certificação. Abstenção
não será chamada de erro. Referências inconclusivas não serão contadas como
acertos e sua quantidade será reportada.

**Disponibilidade:** dados existentes, após revisar a classificação do relatório.

#### Figura 3 — Cobertura certificada por precisão, horizonte e regime

**Como:** mapas de calor: precisão no eixo horizontal, horizonte no vertical e
cobertura na cor; painéis separados por parâmetro logístico e controle afim.

**Complemento:** curva da fração de trajetórias ainda certificáveis até cada
horizonte, tratando corretamente os casos sem abstenção observada. Se apenas
horizontes amostrados estiverem disponíveis, não apresentar o primeiro deles
com abstenção como o instante exato da primeira abstenção por iteração.

**Por que:** mostra onde mais precisão ajuda e onde o caos esgota a escada
autorizada.

**Conclusão possível:** a proposta identifica uma região de decisões
justificáveis; não elimina os limites da dinâmica.

**Disponibilidade:** dados existentes para cobertura por horizonte; conferir a
granularidade das trajetórias antes de interpretar o complemento.

#### Figura 4 — Margem da decisão versus incerteza certificada

**Como:** margem no eixo horizontal e raio total certificado no vertical;
distinguir respostas estáveis, ambíguas e divergências apenas observadas.

Para limiares, usar a posição do enclosure em relação a `tau`: emitir `True`
se o limite inferior for maior ou igual ao limiar, `False` se o superior for
estritamente menor e `Indeterminate` nos demais casos. Para classificação,
certificar a classe `i` somente quando seu limite inferior superar os limites
superiores das concorrentes.

**Por que:** é a figura mais direta para explicar como L2 sustenta L3. Um erro
pequeno pode importar perto da fronteira e ser irrelevante longe dela.

**Limite:** a regra simplificada `margem > 2 epsilon` só será usada quando todos
os logits tiverem o mesmo bound simétrico. A classificação perto da igualdade
usará o enclosure e a convenção de empate, não somente a distância absoluta.

### 3.5. Grupo B — Comparar diretamente contratos e aproximações da literatura

#### Figura 5 — Erro honesto versus liberdade aceita pelo verificador

**Como:** para cada profundidade de composição, apresentar:

1. erro da execução honesta;
2. maior desvio encontrado por busca de witnesses admissíveis;
3. bound global validado.

**Comparar:** relações locais inspiradas em ZIP e modelos de erro relativo,
relações GL e nosso contrato composicional. Identificar a relação exata de cada
baseline; uma adaptação local não será rotulada como ataque ao sistema original.

**Por que:** testa a objeção mais importante à proposta: "se cada operação possui
erro pequeno, por que preciso de um contrato adicional?"

**Cuidados:** toda perturbação deve satisfazer a relação completa, incluindo
domínio e restrições intermediárias. Busca adversarial fornece um **pior caso
encontrado**, não necessariamente o máximo universal. Enumeração exaustiva
ficará restrita a formatos reduzidos. Testar witnesses isolados não substitui a
justificativa matemática do bound global.

#### Figura 6 — Mesma representação, diferentes contratos e métodos

**Como:** fixar formato, arredondamento e domínio; variar Taylor, Padé, GL e
aproximação otimizada por partes/lookup. Mostrar erro observado e bound, quando
disponível, versus ordem e operações. Priorizar exp e sigmoid no domínio já
suportado, sem aplicar GL ao mapa logístico como se este fosse não polinomial.

Adicionar um painel separado para largura e profundidade: número de termos,
profundidade numérica e profundidade da relação de verificação.

**Por que:** demonstra diretamente que "Q16.16" não determina sozinho a semântica.

**Cuidados:** não atribuir uma vantagem de avaliação paralela à representação;
documentar Horner e eventuais reordenações como ablações. A profundidade
constante de Sridhar não será apresentada como medição de constraints Nova.
Máximos observados em malha não serão promovidos a bounds certificados.

#### Figura 7 — Precisão × ordem: quando aumentar bits deixa de resolver

**Como:** mapa de calor de cobertura para

\[
s=\frac14\sum_{i=1}^{4}\operatorname{sigmoid}(z_i),
\qquad z_i\in[-1,1].
\]

Usar precisões `{16,32,48,64}`, ordens GL `{2,4,8,16}` e limiares
`{2/5,1/2,3/5}` (respectivamente `0,4`, `0,5` e `0,6`).

**Por que:** separa o piso de erro da aproximação do erro de quantização. Se a
ordem permanece fixa, aumentar a precisão pode não remover a ambiguidade.

**Comparação central:** precisão fixa, escada sequencial, seleção pela margem
e avaliação local de todos os perfis. A política não pode consultar a resposta
de referência; todas as avaliações locais devem ser contabilizadas.

**Estado:** há infraestrutura off-circuit e checker composicional, mas a
campanha comparativa ainda precisa ser executada. Ver
[0026](0026-cdad-contract-audit.md) e o
[contrato composicional](../cdad-compositional-contract.md).

### 3.6. Grupo C — Demonstrar utilidade e avaliar custo

#### Figura 8 — Preservação da classificação em um modelo MNIST

**Como:** usar uma rede pequena, com camada sigmoid e saída em logits, mantendo
os mesmos pesos e exemplos em todas as variantes numéricas.

Mostrar separadamente:

- acurácia em relação aos rótulos;
- concordância com o modelo de referência;
- cobertura certificada;
- divergências entre classificações definitivas.

**Por que:** leva a proposta além de um exemplo caótico e de uma composição
artificial.

**Cuidados:** preservar numericamente a resposta do modelo não significa que o
modelo acertou o rótulo. Não retreinar uma rede diferente para favorecer cada
representação. Não confundir o modelo matemático canônico com sua implementação
FP32: identificar as duas referências quando ambas forem úteis.

**Estado:** experimento futuro. Arquitetura exata, checkpoint, treinamento,
normalização e protocolo de domínio deverão ser congelados antes da avaliação,
não inferidos retrospectivamente dos resultados.

#### Figura 9 — Onde o custo da adaptação é gasto

**Como:** barras com seleção, avaliações preliminares, witness, vínculo, prova e
verificação. Preparação inicial terá painel separado do custo recorrente
efetivamente medido. Evitar dupla contagem de etapas sobrepostas.

**Comparar:** melhor precisão fixa elegível, escada sequencial, seleção por
margem e avaliação local seguida de uma única prova.

**Por que:** revela se economizar precisão compensa o custo de descobrir qual
precisão usar.

**Limite:** subtrair setup de uma medição não demonstra desempenho com setup
realmente reutilizado. O cenário adversarial curto existente não representa
sozinho o custo sobre uma população de entradas.

#### Figura 10 — Fronteira custo–cobertura

**Como:** cobertura certificada versus custo por oportunidade; painel
complementar com custo por decisão certificada. Incluir intervalos de confiança
nas medições. Se nenhuma decisão for certificada, o custo por decisão não é
finito/definido e não deve aparecer como zero.

**Por que:** responde à pergunta prática: "quanto custa conseguir mais decisões
justificadas?"

**Cuidados:** construir fronteiras separadas para garantias certificadas e
concordância observada. Comparar a mesma população e distinguir tempos
off-circuit de provas reais. Repetir trinta vezes uma única entrada não mede
cobertura populacional. Não converter constraints em tempo alegadamente medido.

### 3.7. Baselines obrigatórios

Além das famílias existentes, incluir na futura comparação:

- **Ponto fixo com o mesmo mecanismo de certificação**, quando aplicável.
  Comparar apenas contra ponto fixo sem bounds confundiria ganho do contrato
  com ganho da representação. Se esse baseline coincidir operacionalmente com
  CBD, registrar a equivalência em vez de inventar uma diferença.
- **Precisão fixa alta** e precisão fixa escolhida em calibração.
- **Seleção local de perfil seguida de uma única prova**, para testar se
  múltiplas reprovas realmente são necessárias.
- **Aritmética intervalar/ball arithmetic convencional**, off-circuit, para
  avaliar qualidade dos bounds e custo numérico, não como novo backend ZK.
- **Aproximações otimizadas por partes e lookups**, evitando baselines
  artificialmente fracos.
- **Predicados simplificados:** para a semântica real, `sigmoid(z) >= 1/2`
  equivale a `z >= 0`; o argmax de softmax equivale ao argmax dos logits.
  Não cobrar uma função transcendental desnecessária para a decisão. Se a
  aplicação exigir saídas arredondadas específicas, explicitar a semântica e
  os empates antes de aplicar a simplificação.

A composição de quatro sigmoids e a camada sigmoid interna de uma rede
continuam relevantes: não possuem, em geral, essa simplificação direta.

### 3.8. Três níveis de comparação com papers

Cada resultado deverá ser identificado como:

1. **Reprodução do artefato oficial:** versões e configurações registradas,
   com validação diferencial.
2. **Implementação local de uma relação publicada:** compara a semântica,
   mas não reproduz o sistema completo.
3. **Extensão nossa:** composição, decisão, adaptação ou nova implementação.

Uma sigmoid inspirada na relação do ZIP não será chamada automaticamente de
reprodução do ZIP. Primeiro será necessário validar uma configuração
efetivamente fornecida por seu
[artefato oficial](https://github.com/vt-asaplab/ZIP).

Tempos publicados em Halo2, Barretenberg ou outros backends não entrarão na
mesma curva de desempenho Nova como se fossem medições homogêneas. Também
serão diferenciadas as hipóteses de privacidade: o pacote dinâmico atual usa
entradas públicas, não demonstra ocultação da entrada.

### 3.9. Protocolo e validações para a execução futura

- Preservar os conjuntos históricos, aleatórios e adversariais separados.
- Usar referência dirigida; reportar referências inconclusivas e excluí-las
  das taxas de concordância, sem ocultar sua frequência.
- Congelar entradas, pesos, arredondamentos, domínios, tolerâncias e políticas
  antes da avaliação final. Registrar seeds, versões, hashes dos perfis,
  artefatos e ambiente.
- No MNIST, calibrar com dados de treinamento e avaliar no conjunto de teste
  sem ajustar os parâmetros aos resultados.
- Não descartar silenciosamente ativações fora do domínio certificado nem
  aplicar clipping que altere o modelo. Reportar falhas de domínio e seu
  impacto na cobertura da população completa.
- Validar coeficientes, quantização, composição e aceitação de witnesses antes
  de interpretar cobertura como certificada.
- Considerar qualquer decisão certificada contrária à referência conclusiva
  um defeito a investigar, não uma taxa aceitável.
- Usar unidades estatísticas adequadas: exemplos/seeds para cobertura;
  rodadas pareadas e intercaladas para variação de tempo. Separar inferência
  estatística de demonstração matemática de inclusão.
- Registrar custos reais no backend local. Sem circuito validado, campos de
  custo ZK permanecem ausentes ou `null`; estimativas, se houver, ficam
  explicitamente identificadas e fora das curvas de tempos medidos.
- Gerar PDF/SVG em Gnuplot exclusivamente dos dados exportados, com legenda
  contendo pergunta, configuração, conclusão permitida e limitação.
- Usar diretórios novos para análises e resultados. Não sobrescrever figuras,
  manifests ou dados históricos para incorporar correções.

## 4. Comandos executados

Foram realizadas apenas inspeções de arquivos, leitura do estado do Git e
verificações documentais. Entre os comandos usados nesta entrega, a partir da
raiz do repositório:

```bash
git status --short
git log -1 --format='%H %s'
sed -n '1,230p' docs/progress/README.md
sed -n '1,240p' docs/thesis-execution-plan.md
sed -n '1,240p' docs/progress/0027-qualification.md
sed -n '1,180p' docs/qualification-results.md
sed -n '128,154p' scripts/plot_qualification.py
git diff --check
git diff --no-index --check /dev/null docs/progress/0028-comparacoes-literatura-e-graficos.md
```

A gravação deste documento e a atualização do índice foram feitas por patch.
Não foram executados comandos de avaliação numérica, treinamento, geração ou
verificação de provas, geração de gráficos, testes Rust/Python ou benchmarks.
Não houve commit nem push nesta etapa.

## 5. Testes, métricas e evidências

### 5.1. Validação desta entrega

Validação exclusivamente documental: revisão do diff, verificação de espaços
e conflitos de patch, existência dos destinos dos links locais e presença das
dez especificações de figuras e das nove seções do diário. Isso não revalida
resultados numéricos, circuitos ou artefatos externos.

### 5.2. Evidência histórica, não reexecutada

O [relatório atual](../qualification-results.md) e o registro
[0027](0027-qualification.md) documentam, para `r=3,9`, limiar `1/2`, horizonte
10 e a amostra de mil entradas:

| Método | Decisões certificadas definitivas | Abstenções | Divergências observadas |
|---|---:|---:|---:|
| Fixed-point RNE F16 | não aplicável | 0 | 8 |
| CBD local F16 | 960 | 40 | 0 |
| CDAD sequencial | 1.000 | 0 | 0 |

Isso demonstra ganho de cobertura de **4 pontos percentuais frente a F16**,
não superioridade frente a F64 nem ganho de tempo. A precisão final média da
CDAD nesse horizonte foi 16,64 bits fracionários.

O mesmo histórico registra 11 decisões definitivas em mil oportunidades no
horizonte 100 e nenhuma no horizonte 1.000, para a CDAD nessa configuração.
Esse colapso de cobertura em horizontes caóticos longos deve permanecer visível.

O registro 0027 também documenta a campanha curta `boundary-10`, com 30 rodadas
no mesmo cenário. A média de tempo completo foi 4,5261 s para F64 fixo,
17,7887 s para a política sequencial e 4,4466 s para preview-all. O histórico
não sustenta uma conclusão de ganho de eficiência da adaptação frente à
precisão fixa alta. Esses valores não são uma medição populacional nem uma
nova campanha deste registro.

Já a auditoria [0026](0026-cdad-contract-audit.md) registra infraestrutura
composicional off-circuit e uma execução pequena de funções. Seu smoke de
funções usou apenas exp/Taylor3/Q8.8/floor; seus gráficos não constituem uma
comparação científica completa entre Taylor, Padé e GL.

## 6. Tentativas malsucedidas

Não houve nova tentativa de execução experimental ou reprodução de papers
nesta entrega; portanto, não há falhas experimentais novas a relatar. A etapa
anterior de pesquisa não gravou o documento por estar em modo de planejamento.
Esta entrega resolve apenas essa pendência documental.

Resultados negativos históricos, inclusive o custo de múltiplas provas e a
abstenção persistente em caos, foram preservados. Não se transformou ausência
de reprodução em evidência contra qualquer paper.

## 7. Limitações conhecidas

- Esta revisão não prova originalidade, nem assegura cobertura exaustiva da
  literatura. A posição editorial de preprints será reconferida antes de
  submeter a tese ou artigo.
- Nenhum artefato externo foi reproduzido nesta etapa. Tempos, memória e
  garantias de backends diferentes não são automaticamente comparáveis.
- Os circuitos atuais e a evidência dinâmica não validam automaticamente as
  novas relações sigmoid em Nova. A comparação off-circuit será identificada
  como tal até existir correspondência validada com o circuito.
- Há uma correção necessária antes de reutilizar a tabela automática de
  custos: o [gerador de gráficos](../../scripts/plot_qualification.py) conta
  respostas definitivas como `certified_decisions` sem exigir, nesse campo,
  pertencimento a uma família certificada. É um problema de classificação do
  relatório, não demonstração, por si só, de erro de soundness da CDAD.
  **A correção foi documentada, não implementada nesta etapa.**
- Uma futura correção deverá diferenciar respostas definitivas observadas de
  certificadas; metadados de certificação ausentes não deverão promover um
  baseline a certificado. Regressões devem cobrir baselines com prova válida
  mas decisão divergente e famílias certificadas com abstenção.
- Zero decisões erradas observadas não substitui uma garantia universal. O
  argumento depende do contrato e da relação efetivamente implementada.
- Menor precisão média não implica menor custo completo. Um bound conservador
  pode reduzir cobertura mesmo quando a execução honesta é precisa.
- Modelos MNIST, distribuições da composição sigmoid e novos custos ainda
  exigem protocolo congelado antes de uma futura execução. Não há resultados
  novos desses workloads neste registro.

## 8. Próximo passo

Ordem recomendada para uma futura etapa de execução:

1. **Consolidar as figuras 1–4 com os dados existentes**, após revisar os
   rótulos e criar análises versionadas novas.
2. **Priorizar as figuras 5–7 como novo resultado científico:** composição de
   erro admissível, margem e cobertura na média de sigmoids.
3. **Validar a utilidade no MNIST**, mantendo modelo e dados congelados e
   explicitando as restrições de domínio.
4. **Avaliar eficiência somente depois**, com comparadores fortes, mesma
   cobertura/semântica de entrada e custos completos.

Critério de conclusão documental deste registro: fontes e atribuições
identificadas, dez figuras especificadas, baselines competitivos definidos,
evidências existentes separadas de hipóteses e índice atualizado. A conclusão
deste marco não significa que as comparações propostas já foram realizadas.

A narrativa para a banca será:

> Mostramos quando uma execução provada diverge da decisão matemática
> pretendida. Nossa proposta explicita e compõe o contrato numérico, permitindo
> decidir somente quando esse contrato sustenta a resposta. Medimos quanto
> isso amplia a cobertura, quanto custa e onde deixa de funcionar.

Essa narrativa deverá ser apoiada apenas pelas partes efetivamente executadas
e verificadas; as novas comparações ainda serão apresentadas como proposta.

## 9. Instrução de continuidade para outra IA

Ler este registro, [0027](0027-qualification.md), o
[relatório de qualificação](../qualification-results.md) e, antes de trabalhar
em sigmoid, [0026](0026-cdad-contract-audit.md) e o
[contrato composicional](../cdad-compositional-contract.md).

Este marco autoriza apenas documentação e já está concluído nesse escopo.
Não interpretar a lista de gráficos como evidência de que eles foram gerados.
Não executar a campanha, alterar o script ou treinar MNIST como continuação
automática de uma solicitação apenas documental.

Quando houver uma solicitação de execução, começar pela classificação da
tabela e pelo inventário dos campos necessários às figuras 1–4. Congelar o
protocolo das comparações novas antes de medir; registrar os próximos avanços
em arquivos numerados novos. Preservar os hashes e registros históricos, a
separação entre garantias e observações, as limitações de domínio/privacidade e
os resultados negativos. Nunca rotular um port local como reprodução de um
paper sem validação diferencial do artefato correspondente.

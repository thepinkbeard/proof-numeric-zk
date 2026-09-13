# Pacote de qualificação: dinâmica, decisões e CDAD

## Pergunta e contrato

A pergunta é se uma execução verificável sustenta o predicado da trajetória
matemática canônica. Uma prova válida de FP32 ou fixed-point não é uma prova de
equivalência à dinâmica real. Divergir dessa dinâmica não invalida o proof system
nem significa que FP32 foi implementado incorretamente.

CBD/CDAD associam à execução um intervalo de inclusão. Um resultado definitivo
só é emitido se o intervalo inteiro determina o predicado. Abstenção é um
resultado legítimo. O objetivo é quantificar garantia, disponibilidade e custo,
não garantir uma vitória de velocidade ou eliminar sensibilidade caótica.

## Reproduzir

Dependências: toolchain Rust compatível com Cargo.lock, Python 3.10+ e Gnuplot
com terminais pdfcairo/svg. Nenhum pacote Python é necessário para gerar os
dados, provar, verificar, analisar tempos ou produzir os gráficos.

```bash
bash scripts/run_qualification.sh results/qualificacao-NOVO quick
bash scripts/run_qualification.sh results/qualificacao-COMPLETA full
```

Diretórios precisam ser novos. `quick` limita entradas/horizontes e usa uma
rodada de demonstração, sem intervalo de confiança. `full` executa 1.007 entradas
(mil aleatórias, uma histórica, seis adversariais), a matriz numérica inteira,
demonstrações curtas e 30 rodadas no cenário primário `boundary-10`. Não executar
outros benchmarks, builds ou testes durante essa medição. A execução completa
pode levar horas e produzir vários GB de dados racionais; preserve espaço livre.

Etapas independentes:

```bash
cargo build --release --locked -p experiment-runner --bin qualification -p nova-circuit --bin nova-circuit
target/release/qualification generate --output-dir results/entradas-NOVO
target/release/qualification evaluate --inputs-dir results/entradas-NOVO --output-dir results/numerico-NOVO
python3 scripts/qualification.py scenarios --output-dir results/cenarios-NOVO
python3 scripts/qualification.py prove --statement results/cenarios-NOVO/boundary-2.json --policy sequential --output-dir results/prova-NOVO
python3 scripts/qualification.py verify --statement results/cenarios-NOVO/boundary-2.json --bundle results/prova-NOVO --output-dir results/verificacao-NOVO
python3 scripts/plot_qualification.py --input-dir results/numerico-NOVO --output-dir results/figuras-NOVO
```

Sem uma campanha fornecida por `--timing-dir`, gráficos de custo mostram
explicitamente que não houve medição. Isso não satisfaz a entrega final de custos.

### Figuras 1–4 do registro 0028: reanalisar dados existentes

O analisador do Grupo A verifica os hashes da matriz, inputs, dados brutos e
perfis, confere os predicados usando os endpoints racionais e exporta somente
tabelas derivadas compactas. A referência usa o enclosure dirigido, nunca só
seu ponto médio. Não reexecuta trajetórias, provas ou timings.

```bash
python3 scripts/qualification_group_a.py prepare \
  --input-dir results/qualification_full_v1 \
  --timing-dir results/qualification_timing_v1 \
  --output-dir results/qualification_group_a_data-NOVO
python3 scripts/qualification_group_a.py render \
  --analysis-dir results/qualification_group_a_data-NOVO \
  --output-dir results/qualification_group_a_figures-NOVO \
  --pdf-output output/pdf/qualification_group_a-NOVO.pdf
python3 -m unittest discover -s scripts -p 'test_qualification_group_a.py' -v
```

A preparação usa apenas a biblioteca padrão Python. A renderização usa
Gnuplot; `--pdf-output` e o teste de concatenação requerem também `pypdf`.
Sem `--pdf-output`, os quatro PDFs/SVGs individuais continuam sendo produzidos.
Os diretórios precisam ser novos. Se os dados históricos não estiverem
disponíveis, primeiro reproduzir a avaliação pelo protocolo original.

Configuração pública da análise: `experiments/qualification_group_a_v1.json`.
`prepare` recusa rejeições, oportunidades faltantes/duplicadas e hashes
inconsistentes; exige o grupo aleatório, sem substituí-lo silenciosamente pelo
histórico. Cada oportunidade pertence a uma categoria de decisão, enquanto a
contagem de contratos definitivos permanece separada da conclusão da referência.
Metadados ausentes nunca promovem um timing a certificado.

`summary.csv/json` mantém conjuntos, parâmetros, inovação afim, horizontes e
famílias separados. CDAD agrega sobre a precisão escolhida, mas preserva os
hashes dos perfis de origem e as médias de precisão/avaliações. `margins.csv/json`
inclui racionais exatos e número da linha de origem para auditoria. Doubles são
usados apenas na apresentação. As trajetórias históricas contêm valores
aproximados de visualização; os marcadores de flips usam as decisões exatas nos
horizontes disponíveis. As figuras não alegam novas provas ZK.

`render` confere os hashes das tabelas antes de produzir `.dat`, `.gp`, PDF,
SVG, relatório e manifesto. Falhas não recebem manifesto de conclusão.
As figuras finais desta rodada e sua leitura estão no
[registro 0029](progress/0029-grupo-a-figuras-auditadas.md).

## Matriz e amostragem

`experiments/qualification_v1.json` fixa parâmetros, horizontes, seeds e
precisões. SplitMix64-v1 usa a transformação inteira especificada no runner;
zero é descartado, e os demais valores são numeradores sobre 2^64. O gerador
tem vetor de teste conhecido. Entradas, matriz e hashes são exportados antes
da avaliação, sem dependência de RNG de uma biblioteca externa.

Os grupos histórico, amostra e adversarial não são agregados entre si. O controle
afim tem limiares 1, 5/4 e 3/2. A matriz é avaliada pelos mesmos geradores de
witness que alimentam os circuitos históricos, sem reimplementar cada formato.
CFD usa p24, raio16, corredor -16:0 no logístico e -24:1 no afim. Casos fora de
suporte são registrados em `rejected.jsonl`, não convertidos em acertos.

O protocolo preserva a inicialização afim histórica por truncamento, mesmo para
operações subsequentes RNE. Isso é parte da semântica pública deste baseline,
não deve ser descrito como RNE em toda a cadeia. CBD/CDAD logístico inicializam
com RNE e raio de uma ULP se a entrada não for representável; caso contrário,
raio zero. Os perfis históricos não são renomeados ou re-hasheados.

Para cada horizonte, a seleção CDAD considera somente intervalos certificados.
Cada avaliação escolhida é recomputada desde x0. A referência não participa da
seleção. `preview-all` avalia quatro perfis e prova só o escolhido; escada e
margem produzem uma prova por tentativa solicitada. O processamento em lote
reutiliza trajetórias dos baselines; seu tempo por trajetória não é custo
isolado de uma oportunidade. A campanha de provas é a fonte de custo completo.

## Referência dirigida

A referência converte entradas e parâmetros com arredondamento dirigido e
propaga intervalos MPFR. Para o logístico, usa intervalos positivos de
`r*x*(1-x)` e a invariância demonstrável de [0,1] para r em [0,4]. Para a afim,
usa os sinais positivos dos coeficientes. A precisão inicia em max(256,2N+128)
e aumenta até 16.384 bits se alguma decisão continuar inconclusiva.

O erro central exportado usa o ponto médio da referência como medida observada;
a classificação da decisão usa o intervalo inteiro. Enclosures de referência
e de representação disjuntos invalidam a execução. Sobreposição não é, por si,
prova de inclusão: essa obrigação vem das regras matemáticas dos contratos,
dos testes exatos reduzidos e de sua correspondência aos circuitos. Não se
confunde esse teste experimental com uma demonstração universal.

## Provas vinculadas de entrada pública

Contrato novo: `public-canonical-rne-v1`. A declaração autorizada contém x0,
r, limiar e horizonte. Ela deve ser fornecida ao verificador por caminho
independente; não se aceita simplesmente a declaração que o prover escolher.

Cada prova comprimida Nova demonstra uma execução em precisão fixa. O verificador
Rust reconstrói public parameters/verifier key a partir da configuração autorizada,
confere o estado inicial e verifica a prova salva. Não carrega uma verifier key
arbitrária fornecida no pacote. Depois confere o estado final autenticado.

O verificador do pacote faz uma segunda checagem exata, com racionais, da
quantização RNE, limbs, indicador x=1, raio inicial, contador e precisão. Confere
a igualdade da entrada/parâmetros/horizonte entre provas e a decisão final.
Os estados e a entrada são públicos: checar essa fronteira fora do circuito
é suficiente porque z0 é argumento público autenticado de `verify`. Não se
afirma que um novo gadget privado de quantização foi implementado.

O SHA-256 da declaração identifica o vínculo, mas a comparação também usa
seus campos completos. Não é o compromisso Poseidon privado do protocolo legado.
O conjunto de provas é a entrega adaptativa; não há folding heterogêneo.

O verificador confirma a declaração e cada prova fornecida, não a honestidade
de tempos, previews ou uma alegação de minimalidade da precisão. A política
é avaliada pelo harness controlado e não constitui a premissa de soundness.

Casos de fronteira são construídos antes dos testes: aplicar N vezes a inversa
inferior de `4x(1-x)` ao limiar 1/2 em Decimal100, quantizar na grade 2^-64 e
somar 1024 ULP dessa grade. O procedimento está versionado; não seleciona casos
depois de observar desempenho.

## Custo e inferência

A campanha inclui quatro precisões fixas CDAD, três políticas adaptativas e
baselines locais: fixed-truncate32, fixed-rne32, CBD32, CBD-sequencial32,
CFD24 e FP32 restrito. Todos usam Nova0.73/Pallas-Vesta e o mesmo cenário.

O tempo completo inclui processos, seleção, witnesses, setup, provas,
serialização e verificação independente. Setup de prover e verificador é
reportado separadamente; sua subtração produz uma métrica de componentes sem
setup, **não** uma execução demonstrada com setup reutilizado. O custo de vínculo
público está incluído no verificador do pacote, não se soma o custo do gadget
Poseidon legado. Contagens da etapa e do circuito aumentado permanecem separadas.

Há aquecimento e 30 rodadas pareadas, com ordem embaralhada por seed fixo. O
cenário primário de timing é `boundary-10`; os quatro cenários possuem também
demonstração curta. O bootstrap pareado usa 10.000 reamostragens das rodadas,
seed20260911, percentis 2,5/97,5. Comparações múltiplas são exploratórias e
identificadas como não ajustadas. Não extrapolar uma diferença nesse cenário
para vantagem populacional. Comparações de eficiência certificada exigem
mesma cobertura; acerto observado de um formato sem bound não é certificado.

A campanha concluída em `results/qualification_timing_v1` tem 403 registros
(aquecimento mais 30 rodadas, 13 configurações) no cenário `boundary-10`.
Os tempos médios completos foram aproximadamente 4,53 s para fixed F64,
4,45 s para preview-all F16 e 17,79 s para as políticas que geraram múltiplas
provas (sequential/margin-directed). Assim, esta execução não demonstra redução
de custo da adaptação frente à precisão fixa alta; ela demonstra o custo real de
obter a cobertura adaptativa neste cenário. A análise é exploratória, específica
do host e deve ser lida junto do `analysis.json`, não como benchmark universal.

## Relação com trabalhos primários

Revisão direcionada conferida em 11/09/2026, não revisão sistemática exaustiva.

| Trabalho | Garantia/tema relevante | Relação com esta entrega |
|---|---|---|
| [Garg, Jain, Jin e Zhang, CCS2022](https://par.nsf.gov/servlets/purl/10408517) | Modelo de correção aproximada com erro relativo | Contextualiza contratos aproximados; CFD local não é reprodução desse protocolo |
| [Ernstberger et al., S&P2025](https://discovery.ucl.ac.uk/id/eprint/10211442/) | Circuitos floating-point e privacidade de localização | Motiva baseline de semântica especificada; nosso FP32 é restrito e não reproduz integralmente ZKLP |
| [Sridhar et al., USENIX2026](https://www.usenix.org/conference/usenixsecurity26/presentation/sridhar) | Aproximações não polinomiais, GL, quantização e profundidade limitada; Noir/Barretenberg | Influência sobre contratos; GL não é baseline do mapa logístico polinomial |
| [Zamir, preprint2026](https://arxiv.org/abs/2602.15756) | Contraexemplo à inferência local-global para verificação aproximada de redes | Motiva explicitar composição adversarial; não é teorema específico do mapa logístico |
| [Appel e Kellison, VCFloat2, CPP2024](https://www.cs.princeton.edu/~appel/papers/vcfloat2.pdf) | Bounds de arredondamento e certificados em Coq | Referência de rigor; o projeto não reivindica prova mecanizada equivalente |
| [Shewchuk, 1997](https://www.cs.cmu.edu/~quake/robust.html) | Precisão adaptativa para predicados geométricos | Antecedente que impede alegar novidade da adaptação isolada |

A contribuição candidata é a integração de contrato, decisão, adaptação e
verificação de uma execução dinâmica. Gráficos são comparações homogêneas das
implementações locais. Nenhum número de desempenho de outro backend é inserido
como se tivesse sido medido aqui.

### Matriz de comparação semântica

As referências são as fontes primárias ligadas acima. "Não reproduzido" impede
atribuir os números locais ao artefato do trabalho. Esta tabela delimita a
comparação; não pretende listar todas as funcionalidades de cada sistema.

| Sistema | Representação e operação | Contrato | Composição | Decisão | Backend nesta comparação | Restrição relevante |
|---|---|---|---|---|---|---|
| Garg et al. | Floating-point aproximado, operações aritméticas | Erro relativo permitido pela relação | O modelo numérico é parte da garantia; não se infere preservação arbitrária de predicados | Não reproduzida aqui | Não reproduzido | CFD local não equivale ao protocolo dos autores |
| ZKLP, Ernstberger et al. | Floating-point; artigo declara conformidade IEEE754 | Semântica especificada de ponto flutuante | Aplicação geográfica construída com os circuitos | Proximidade/localização | Não reproduzido | Conformidade IEEE não implica igualdade à dinâmica real |
| Sridhar et al. | Fixed-point quantizado, aproximações racionais e relações implícitas GL | Erro de aproximação, quantização e tolerância da relação | Exige acompanhar o erro da cadeia numérica; profundidade da verificação é outra medida | Não é a adaptação CDAD orientada ao predicado | Noir/Barretenberg no artigo; não reproduzido nesta matriz dinâmica | Domínios e parâmetros específicos; logístico não precisa de GL |
| Baseline FP32 local | FP32 restrito, ordem das operações documentada | Semântica do circuito local, sem enclosure da trajetória real | Iteração da recorrência representada | Predicado da trajetória FP32 | Nova0.73/Pallas–Vesta | Não é implementação IEEE irrestrita nem reprodução de ZKLP |
| CBD/CDAD desta entrega | Centro–raio diádico, RNE logístico | Inclusão conservadora sob domínio e regras públicas | Propagação do raio em cada passo | True/False somente quando o intervalo separa o limiar; senão abstenção | Nova0.73/Pallas–Vesta | Provas curtas, entrada pública, sem folding heterogêneo |

## Limites da alegação para a banca

- Garantia numérica e zero falhas observadas são evidências diferentes.
- FP32 correto pode divergir da dinâmica real; isso não refuta IEEE nem o paper
  cujo objetivo seja provar exatamente essa semântica.
- Aumento de cobertura por adaptação pode custar mais que precisão fixa alta.
- Abstenção persistente em caos é um limite informativo, não um fracasso oculto.
- As provas curtas demonstram execução criptográfica; 1.000 passos da matriz
  numérica não devem ser apresentados como 1.000 folds provados nesta campanha.
- A comparação não cobre privacidade da entrada nem circuitos Nova heterogêneos.

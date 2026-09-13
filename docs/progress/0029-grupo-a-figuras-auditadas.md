# 0029 — Grupo A: figuras auditadas de decisões em sistemas dinâmicos

Data: 2026-09-13. Estado: primeira etapa de execução do
[registro 0028](0028-comparacoes-literatura-e-graficos.md) concluída.
Figuras 1–4 produzidas; figuras 5–10 e os experimentos sigmoid/MNIST continuam
pendentes. Entrega aditiva, sem sobrescrever resultados históricos.

## 1. Objetivo e estado

Corrigir a classificação de decisões no relatório automático e transformar os
dados existentes em quatro figuras explicáveis para a qualificação:

1. trajetória representada versus referência, em regimes estável e caótico;
2. decisões certificadas, concordâncias observadas, divergências e abstenções;
3. cobertura certificada por precisão, horizonte e regime;
4. margem da decisão versus raio certificado ou erro observado.

Foi feita uma reanálise integral dos endpoints e decisões já exportados. Não
foram executadas novas trajetórias experimentais, treinamento MNIST, reprodução
de papers ou campanhas de provas/tempos. A suíte existente foi executada como
validação de regressão; isso não é uma nova medição experimental de eficiência.

## 2. Arquivos e artefatos

### Código e documentação

- `scripts/qualification_metrics.py`: classificação compartilhada e checagem
  racional dos predicados; contagens de timing sem promoção implícita a
  certificação.
- `scripts/plot_qualification.py`: correção da tabela e da fronteira de timing;
  número de rodadas obtido do manifesto, não texto fixo em 30.
- `scripts/qualification_group_a.py`: comandos `prepare` e `render`, tabelas
  derivadas, hashes, verificação de população e figuras Gnuplot.
- `scripts/test_qualification_group_a.py`: 30 testes novos, incluindo pipeline
  com fixtures sintéticas e exportação PDF/SVG.
- `experiments/qualification_group_a_v1.json`: configuração pública da análise.
- README, [guia de qualificação](../qualification.md), índice do diário e este
  novo registro. Os registros concluídos, incluindo 0028, não foram reescritos.

### Saídas desta entrega

- **PDF final:** [quatro figuras](../../output/pdf/qualification_group_a_v2.pdf).
- **Leitura das figuras e tabelas:**
  [relatório gerado](../../results/qualification_group_a_figures_v2/report.md).
- **Figuras individuais PDF/SVG e scripts Gnuplot:**
  `results/qualification_group_a_figures_v2/`.
- **Tabelas JSON/CSV e racionais de margem:**
  `results/qualification_group_a_data_v1/`.
- **Reprodução independente da preparação:**
  `results/qualification_group_a_data_replay_v1/`.

Os dados derivados ocupam aproximadamente 20 MB por preparação, sem duplicar
os cerca de 4,9 GB do diretório numérico histórico. As pastas regeneráveis em
`results/qualification*/` continuam ignoradas pelo Git conforme a política
existente; precisam ser reproduzidas se não estiverem presentes em outro clone.
O código, a configuração, a documentação e o PDF final são arquivos disponíveis
para versionamento. Esta etapa não fez commit nem push.

## 3. Decisões e hipóteses

### Integridade e classificação

- Conferir o SHA-256 de `raw.jsonl` durante sua leitura sequencial, além de
  hashes de matriz e entradas. Reconstruir o hash de cada perfil no esquema
  histórico sem modificar esse esquema.
- Verificar conjuntos, identificadores, contagem e presença de todas as
  oportunidades por entrada; rejeitar duplicatas, lacunas e configurações não
  autorizadas pela matriz histórica.
- Recalcular `True/False/Indeterminate` diretamente dos endpoints racionais,
  tanto para o resultado quanto para a referência. Não decidir a partir do
  valor float do centro.
- Rejeitar inconsistência de metadados, contradição certificada e disjunção
  entre enclosures certificados e de referência. Essa verificação não constitui
  uma prova universal de inclusão: sobreposição não basta para tal alegação.
- Particionar cada oportunidade em uma categoria. Referência inconclusiva
  tem precedência na partição gráfica; a contagem de contratos definitivos e
  todas as abstenções também ficam disponíveis separadamente.
- Em timing, `certified_family` ausente não autoriza certificação. Uma família
  certificada reconhecida ainda precisa de decisão definitiva; acerto observado
  e prova válida não promovem um baseline sem contrato.

### Desenho das figuras

| Figura | Dados e configuração | Conclusão permitida | Limitação |
|---|---|---|---|
| 1 — Trajetórias | x0=1/10 histórico, r=3,5 e 3,9, até 100 passos, limiar 1/2; fixed trunc./RNE F16, FP32 e CBD F16 | Representação e operações afetam a trajetória e o predicado | Curvas são visualizações aproximadas. Marcadores de flips vêm dos registros exatos nos horizontes avaliados; não são novas provas ZK |
| 2 — Decisões | 1.000 entradas aleatórias por horizonte, r=3,9, limiar 1/2; dez configurações/políticas | Distinguir resposta certificada, concordância observada, erro e abstenção | Trinta repetições de timing não entram nessa população; conjuntos adversariais e histórico não se misturam à amostra |
| 3 — Cobertura | CBD local F16/32/48/64; quatro regimes logísticos e dois controles afins | Mais precisão recupera cobertura em algumas regiões, mas não elimina o limite em caos longo | Cobertura terminal não é sobrevivência contínua nem instante da primeira abstenção; afim preserva inicialização histórica |
| 4 — Margem | 5.000 oportunidades por painel, todos os horizontes da amostra em r=3,9, limiar 1/2 | A decisão depende da relação entre incerteza e distância à fronteira | Raio certificado e erro observado ficam em painéis distintos; eixo vertical logarítmico, sem subamostragem |

A figura 4 usa margem horizontal linear, evitando ocultar casos de margem zero.
O piso visual vertical é `1e-20`; nenhum dos 20.000 pontos desta execução ficou
abaixo dele. As decisões não usam esse piso. A diagonal é `y=margem`, não uma
certificação para os baselines sem bound. `F` indica bits fracionários, enquanto
`p24` de FP32/CFD indica precisão do significando.

Cada política CDAD reúne todas as precisões selecionadas na mesma população.
A tabela preserva a lista dos hashes de perfil de origem, precisão média e
número médio de avaliações, sem apresentar precisão selecionada como nova
população independente.

## 4. Comandos executados e reprodução

Nesta máquina, foi utilizado o Python do runtime disponível em
`/Users/barbosa/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3`.
Nos comandos abaixo, `python3` representa esse interpretador; outro ambiente
precisa de Python 3.10+, Gnuplot com pdfcairo/svg e `pypdf` para concatenar PDFs.

```bash
python3 -m unittest discover -s scripts -p 'test_qualification_group_a.py' -v
python3 scripts/test_qualification.py -v
cargo test --workspace --locked --quiet

python3 scripts/qualification_group_a.py prepare \
  --input-dir results/qualification_full_v1 \
  --timing-dir results/qualification_timing_v1 \
  --output-dir results/qualification_group_a_data_v1

python3 scripts/qualification_group_a.py prepare \
  --input-dir results/qualification_full_v1 \
  --timing-dir results/qualification_timing_v1 \
  --output-dir results/qualification_group_a_data_replay_v1

python3 scripts/qualification_group_a.py render \
  --analysis-dir results/qualification_group_a_data_v1 \
  --output-dir results/qualification_group_a_figures_v2 \
  --pdf-output output/pdf/qualification_group_a_v2.pdf

pdftoppm -scale-to 1400 -png output/pdf/qualification_group_a_v2.pdf \
  tmp/pdfs/qualification_group_a_v2/page
pdfinfo output/pdf/qualification_group_a_v2.pdf
git diff --check
```

Os diretórios da rodada acima já existem. Para repetir, escolher **novos
diretórios e nome de PDF**. Foi também renderizada uma versão preliminar `v1`
antes dos ajustes visuais. As imagens de revisão foram movidas das pastas
temporárias para a subpasta `qa/` de cada diretório de figuras, sem apagá-las.

A preparação é independente da renderização. Os manifests registram os hashes
do código que executou cada etapa; a renderização final usa os mesmos dados
auditados da primeira preparação, com o layout corrigido.

## 5. Testes, métricas e evidências

### Validação executada

- **172 testes Rust passaram** na suíte existente do workspace.
- **30 testes novos Python passaram**, incluindo limites exatos, empates,
  decisões abaixo da resolução de float, abstenção, referência inconclusiva,
  metadados ausentes/forjados, hash de perfil histórico, duplicatas, hash bruto
  adulterado, bloqueio de overwrite e hash de dados derivados adulterado.
- O teste do relatório legado usa uma rodada sintética e confere tanto a
  contagem corrigida quanto a ausência do texto fixo "30 rodadas".
- Os três testes públicos independentes de provas da suíte Python existente
  passaram. Seus onze testes opcionais com pacotes reais foram ignorados por
  não fornecer `QUAL_BUNDLE` e `QUAL_STATEMENT`; não são novos testes de
  adulteração criptográfica executados nesta rodada.
- **1.782.390 registros**, **1.007 entradas**: 1.000 aleatórias, uma histórica e
  seis adversariais. Todas as oportunidades foram auditadas duas vezes, sem
  inconsistência detectada, contradição certificada ou referência inconclusiva.
- **11 arquivos de dados derivados idênticos byte a byte** entre preparação
  e reprodução. Manifests têm proveniência própria e não são usados nessa
  igualdade, pois registram ambiente/código da etapa.
- **5.310 grupos coincidiram** com uma conferência independente do resumo
  histórico: oportunidades, decisões certificadas definitivas, flips e
  abstenções; linhas adaptativas históricas foram recombinadas por política.
- Os quatro PDFs/SVGs foram gerados. O PDF consolidado tem quatro páginas;
  todas foram renderizadas e inspecionadas visualmente. O teste de PDF também
  confere que a nota de rodapé da primeira página aparece uma vez, sem
  sobreposição causada pelo multiplot.

### Resultados reconfirmados

Amostra aleatória, r=3,9, limiar 1/2; cada linha possui mil oportunidades:

| Horizonte | Flips fixed RNE F16 | Definitivas CBD F16 | Definitivas CBD F64 | Definitivas CDAD escada |
|---:|---:|---:|---:|---:|
| 10 | 8 | 960 | 1.000 | 1.000 |
| 30 | 507 | 0 | 1.000 | 1.000 |
| 100 | 502 | 0 | 11 | 11 |
| 300 | 471 | 0 | 0 | 0 |
| 1.000 | 491 | 0 | 0 | 0 |

O ganho de cobertura frente a F16 permanece demonstrado. **Não é um ganho de
cobertura frente a F64 nem uma evidência de menor tempo.** O limite caótico
permanece explícito, em contraste com os controles afins.

Na tabela corrigida de timing, `baseline-fixed-truncate`, F32, cenário
`boundary-10`, possui **0 decisões certificadas**, **30 respostas definitivas
sem contrato certificado** e **30 flips** nas repetições. Os dados originais
não mudaram: mudou a classificação incorreta que chamava essas respostas de
certificadas. Esse caso não demonstra falha criptográfica do Nova.

### Identificação dos artefatos

| Artefato | SHA-256 |
|---|---|
| `results/qualification_full_v1/raw.jsonl` | `a78e1e459adb6bb02b5785228b92db0e9891cb440e03e3e585b5519c92bc5c38` |
| `results/qualification_group_a_data_v1/manifest.json` | `aa53b46688e91dbedd8f71da5a5657c73d3a1a935a70fe9635090af406e52a73` |
| `results/qualification_group_a_figures_v2/manifest.json` | `b28c87006001a54bca26ce34875d283054cd5d84fddedefc4bc8dbab98312aa6` |
| `output/pdf/qualification_group_a_v2.pdf` | `fdf3695fef6fbce31dc4dd6e426f1b764014f520b7006349ec88b28a222c5bf7` |

## 6. Tentativas malsucedidas e ajustes

- Na revisão visual inicial, a legenda da figura 1 ficava muito baixa. Foi
  reposicionada uma única vez acima dos painéis.
- Labels de rodapé em coordenadas de tela eram redesenhados em cada subplot,
  deixando o texto mais escuro e duplicado na extração do PDF. O rodapé passou
  a ser desenhado uma única vez. A versão visual corrigida é `v2`; `v1` é
  preliminar e foi preservada.
- As fixtures dos testes são explicitamente sintéticas, apenas para testar
  importação, classificação e exportação. Não integram nenhuma taxa experimental.

Não houve falha da análise de dados reais ou tentativa de reproduzir um paper
externo nesta etapa.

## 7. Limitações conhecidas

- Conferir um predicado a partir dos endpoints exportados não prova que todos
  os endpoints são bounds corretos de todas as possíveis execuções. Essa
  obrigação permanece nos contratos, nos testes numéricos e na relação dos
  circuitos com esses contratos.
- As figuras não são benchmark executado contra ZIP, Sridhar, ZKML ou outro
  artefato publicado. O posicionamento bibliográfico continua no registro 0028.
- Trajetórias longas são off-circuit; demonstrações criptográficas históricas
  possuem escopo curto e entradas públicas. Nenhuma alegação nova de
  privacidade, provas heterogêneas ou eficiência foi acrescentada.
- A figura 3 não calcula uma curva de primeira abstenção. Isso exigiria tratar
  explicitamente censura e a granularidade disponível, em análise própria.
- Todos os racionais originais permanecem na origem; exportações float para
  gráficos não substituem esses registros. Referências inconclusivas são
  suportadas pela classificação, mas não ocorreram na matriz histórica auditada.
- As medições de tempo apenas foram reclassificadas. Não houve nova inferência
  estatística nem generalização do cenário `boundary-10` para uma população.

## 8. Próximo passo

O próximo resultado novo é o Grupo B, figuras 5–7: erro honesto versus liberdade
aceita, comparação de aproximações na mesma representação e mapa precisão ×
ordem para a média de quatro sigmoids. Começar pelo contrato/checker existente
de 0026, congelar distribuição de entradas e tolerâncias e validar os baselines
antes da campanha. Depois disso, avançar para MNIST e custo completo.

Esta etapa conclui **somente o Grupo A**. A pesquisa de 0028 não se tornou,
por isso, uma reprodução experimental de todos os papers citados.

## 9. Continuidade para outra IA

Usar o PDF `qualification_group_a_v2.pdf` e seu relatório, não a versão visual
preliminar. Conferir os hashes e preservar os dados históricos. Para regenerar,
usar `prepare` e `render` com destinos novos; não usar os diretórios desta rodada
como alvos de overwrite.

Ler [0028](0028-comparacoes-literatura-e-graficos.md),
[0026](0026-cdad-contract-audit.md) e
[cdad-compositional-contract.md](../cdad-compositional-contract.md) antes do Grupo
B. Não confundir fixtures de teste, reanálise de resultados antigos, execução
off-circuit e prova verificada. A política afeta custo; a validade da decisão
depende do contrato. Preservar resultados negativos e explicitar quando
precisão fixa alta já oferece a mesma cobertura.

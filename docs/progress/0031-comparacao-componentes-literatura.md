# 0031 — Comparação executada com componentes da literatura

## 1. Objetivo e estado

**Campanha numérica F16 concluída, com seis figuras e reprodução verificada.**
Data: 2026-09-18. Git local/remoto conferidos em `7826cb4`; nenhum pull era
necessário. Arquivos auxiliares do usuário foram preservados. Não fizemos commit
ou push desta entrega.

A comparação agora usa componentes oficiais de **Sridhar/USENIX Security 2026**,
**ZKML/EuroSys 2024** e **NFGen/CCS 2022**, em vez de apenas atribuir nomes da
literatura a implementações locais. Isso **não** equivale à reprodução integral
de três sistemas de provas: NFGen é MPC; Halo2/Barretenberg não foram executados
como provers. Noir executou as relações originais de Sridhar para validar witnesses.

Usar o [PDF consolidado v3](../../output/pdf/literature_comparison_v3.pdf) e as
[tabelas finais](../../results/literature_comparison_v3/summary.csv).
O [protocolo](../literature-comparison-protocol.md) explica fontes, fórmulas,
adaptações e limites. Não substituir os Grupos A/B anteriores por esta campanha.

## 2. Arquivos e artefatos

- `experiments/literature_sources_v1.json`: versões completas, fontes e origem.
- `experiments/literature_comparison_v1.json`: entradas, precisão, limiares e seeds.
- `scripts/literature_artifacts.py`: execução dos geradores oficiais, compilação
  do corpo original da tabela ZKML, tradução modular e checagem Noir.
- `scripts/literature_compare.py`: geração, contratos de tabela, composição,
  replay de seleção e verificação em processo separado.
- `crates/experiment-runner/src/bin/literature_bridge.rs`: MPFR dirigido,
  constantes do perfil histórico e checker Rust independente do Python.
- `SigmoidRelationContract::check_batch`: nova interface aditiva que reutiliza
  somente o bound público dos momentos; todos os checks individuais permanecem.
- `scripts/plot_literature_comparison.py`, `scripts/test_literature_comparison.py`,
  `scripts/run_literature_comparison.sh` e dependências Python fixadas.
- README, protocolo e índice de progresso.

Dados completos locais, regeneráveis e fora do Git pelo padrão existente:

- `results/qualification_literature_v1/`: primeira campanha, preservada.
- `results/qualification_literature_v2/`: **campanha principal**, inclui CSV e
  validação cruzada de todos os certificados Python/Rust.
- `results/qualification_literature_v2_replay/`: reprodução independente.
- `target/literature-artifacts/`: clones fixados e compilador Noir isolado.

Artefatos compactos para o repositório: `results/literature_comparison_v3/`
contém seis PDF/SVG individuais, scripts Gnuplot, dados derivados, resumo e
manifesto. `output/pdf/literature_comparison_v3.pdf` contém as seis páginas.
Versões gráficas v1/v2 são preliminares preservadas; **usar v3**.

## 3. Decisões e hipóteses

### O que foi realmente reproduzido

1. **Sridhar:** os quatro geradores foram executados sem alteração, com um
   adaptador explícito para fornecer as mesmas entradas. Graus/ordens são os
   escolhidos no `run_all.py` final-art para F16. Seus ramos de relação foram
   executados em Noir beta.21 com entrypoint isolado e constantes geradas.
2. **ZKML:** o corpo original de `generate_map` foi extraído e compilado em Rust.
   Uma implementação Python comparou todas as saídas do subconjunto usado.
   O checker de pertencimento à tabela é local; **não é execução Halo2**.
3. **NFGen:** execução de `SampledPolyFit` e do preditor oficial. A escolha entre
   candidatos é nossa, pelo número de coeficientes entre os viáveis. **Não**
   reproduz o profiler de custo MPC nem MP-SPDZ. Seu emulador decimal é preservado.

Os quatro geradores Sridhar produzem a saída honesta quantizando a função de
alta precisão; não calculam quatro resultados honestos diferentes pela avaliação
de aproximações. Por isso suas curvas de erro coincidem. A relação aceita pode
ser diferente, como a busca mostra. Nossa raiz implícita GL8 é outra execução,
identificada como extensão.

### O que acrescentamos

Para ZKML e o emulador NFGen congelado, enumeramos **todas as 65.537 entradas
F16 de [0,1]**, com referência dirigida. Somamos o bound de quantização de entrada
via Lipschitz da sigmoid. Isso produz um contrato global no escopo declarado,
não um máximo observado apenas nos 1.025 pontos da malha experimental.

Para nosso GL8, os 20.148 certificados foram conferidos por Python e Rust.
O bound do contrato cobre qualquer witness aceito pela **nossa relação racional**.
Não transferimos esse resultado ao circuito de Sridhar nem à execução MPC do NFGen.

A campanha compara 1.000 vetores aleatórios, 14 adversariais separados, 5.037
inputs escalares distintos e dez perfis. Cada vetor gera três predicados: as
3.000 oportunidades aleatórias são correlacionadas por vetor.

## 4. Comandos executados e reprodução

```sh
cargo build --release --locked -p experiment-runner --bin literature-bridge
target/literature-venv/bin/python scripts/literature_compare.py generate \
  --run-dir results/qualification_literature_v2
target/literature-venv/bin/python scripts/literature_compare.py verify \
  --run-dir results/qualification_literature_v2
target/literature-venv/bin/python scripts/literature_compare.py generate \
  --run-dir results/qualification_literature_v2_replay
target/literature-venv/bin/python scripts/plot_literature_comparison.py \
  --run-dir results/qualification_literature_v2 \
  --output-dir results/literature_comparison_v3 \
  --pdf output/pdf/literature_comparison_v3.pdf
cargo test --workspace --locked
target/literature-venv/bin/python -m unittest discover -s scripts -p 'test_qualification*.py'
target/literature-venv/bin/python -m unittest discover -s scripts -p test_literature_comparison.py
GROUP_B_RUN=results/qualification_group_b_pilot_v1 \
GROUP_B_REPLAY=results/qualification_group_b_pilot_replay_v1 \
target/literature-venv/bin/python -m unittest discover -s scripts -p test_group_b_artifacts.py
```

O comando de uma entrega nova é:

```sh
bash scripts/run_literature_comparison.sh results/qualification_literature_NOVO
```

## 5. Evidências, testes e resultados

### Validação

- **177 testes Rust passaram**, incluindo o novo teste de equivalência do
  checker em lote. Serialização/hashes históricos permanecem cobertos.
- **16 testes novos Python passaram**, incluindo execução Noir positiva/negativa,
  coeficientes conhecidos, entradas, intermediários, perfil, raio, decisão e
  checagem exaustiva da tabela ZKML.
- Suíte `test_qualification*.py`: **41 passaram, 11 opt-in não executados**
  (requerem caminhos de provas reais; nenhuma nova campanha de provas foi alegada).
- **8 testes de artefatos históricos do Grupo B passaram**, incluindo adulteração
  após rehash e reprodução do piloto antigo.
- Verificação da campanha em outro processo: **50.370 linhas escalares**,
  **30.420 decisões**, **zero contradições certificadas**, **zero provas novas**.
- **19 arquivos determinísticos idênticos byte a byte** entre v2 e replay,
  incluindo inputs, referências, dados CSV/JSONL, witnesses, tabelas, decisões e
  políticas. Tempos/proveniência/logs de execução não entram nessa exigência.
- Noir: quatro lotes honestos, quatro controles negativos e quatro lotes de
  witnesses extremos da busca; todos coincidem com a classificação local.
- PDF/SVG produzidos por Gnuplot, com inspeção visual das seis páginas.

### Cobertura observada nesta população

| Configuração | Certificadas / 3.000 aleatórias | Certificadas / 42 adversariais |
|---|---:|---:|
| ZKML lookup + nosso contrato de tabela | 3.000 | 30 |
| NFGen emulador + nosso contrato de tabela | 2.996 | 30 |
| Nosso GL8 F16 | 2.998 | 30 |
| Nosso GL8 F32 | 3.000 | 36 |
| Nosso GL8 F48 | 3.000 | 36 |
| Nosso GL8 F64 | 3.000 | 36 |
| Nosso GL8 adaptativo, escada/margem/preview | 3.000 | 36 |

Para as relações modulares nativas de Sridhar, **não estabelecemos nesta
campanha um contrato global para todos os witnesses**. Por isso a tabela acima
não apresenta “0% de cobertura do artigo”. Seus resultados são comparados como
concordância/divergência observada e aceitação de witnesses.

Todos os perfis tiveram zero divergências honestas nas oportunidades aleatórias.
Nos adversariais, cada configuração F16 teve **6 divergências honestas em 42**.
Os contratos de tabela e nosso GL8 F16 abstiveram-se nesses casos, além de
outros seis casos corretos mas próximos demais da fronteira. Isso evidencia o
compromisso entre cobertura e justificativa, não uma melhoria do valor calculado.

**O lookup é um baseline forte:** já atinge todas as oportunidades aleatórias
em F16. Não há superioridade geral de CDAD sobre ZKML nessa distribuição.

### Adaptação

Na amostra aleatória, a escada usou F16 em 2.998 oportunidades e F32 nas duas
restantes: média de **1,000667 perfis avaliados**. Preview-all avaliou quatro
perfis em todas. Nos adversariais, escada e margem alcançaram 36/42; médias de
avaliações **1,571429** e **1,476190**, respectivamente. A seleção por margem
pode saltar além da menor precisão suficiente.

As seis abstenções adversariais persistentes não desaparecem em F64 com ordem
GL8 fixa. O contrato inclui erro de aproximação, não apenas quantização. Não
afirmamos impossibilidade matemática de certificar esses inputs por outro método.

### Witnesses alternativos aceitos

Busca limitada em cinco entradas, ±256 ULP, seguida de validação dos extremos
no Noir original. Maior desvio encontrado, não máximo universal:

| Relação Sridhar F16 | Maior desvio encontrado |
|---|---:|
| GL4 | 1,52587890625e-5 |
| Poly grau 5 | 2,14149461218e-3 |
| Padé [3/3] | 6,103515625e-5 |
| PWL 8 segmentos | 2,44140625e-4 |

As diferenças não indicam provas criptograficamente falsas: são saídas aceitas
pelas relações implementadas. A comparação motiva analisar a relação completa,
não apenas o erro do algoritmo que gera witnesses honestos.

## 6. Tentativas e ajustes

- NFGen exigiu dependências adicionais `pandas`/`dill`, agora fixadas. O clone
  possui arquivos de profiler com nomes que colidem em sistemas sem distinção
  de maiúsculas/minúsculas; esses pickles **não são consumidos** nesta campanha.
- O primeiro extrator da função Rust assumia quatro espaços de indentação;
  foi corrigido para extrair a assinatura e delimitar chaves, sem editar o corpo.
- A primeira campanha não tinha CSV nem verificação Rust de todo o lote;
  v2 foi criada em diretório novo para acrescentá-los, sem apagar v1.
- A revisão visual corrigiu notação científica, espaçamento e a legenda da
  busca: o Noir validou os **20 extremos selecionados**, não todos os candidatos.
  Figuras preliminares v1/v2 foram preservadas; usar o PDF v3.

## 7. Limitações

- Comparação de componentes na configuração F16, não sweeps completos dos três
  sistemas. O sweep F16/32/48/64 desta entrega é da nossa relação GL8.
- Sem novas provas ZK, sem execução Halo2 ou MPC, sem comparação homogênea de
  constraints, memória ou tempo de proving. Noir execute não é prover/verifier.
- O gráfico de recursos conta constantes, não estima constraints. O subconjunto
  usado da tabela ZKML não é a alocação inteira do HashMap.
- O contrato NFGen certifica somente o emulador congelado em uma grade finita.
- Nenhuma conclusão de ausência de garantia em Sridhar decorre de `bound=null`.
- A campanha não usa MNIST, inferência treinada ou dados rotulados; é um
  benchmark controlado. Não mede acurácia de classificação contra rótulos.
- Nos adversariais as quatro coordenadas são iguais: servem para estressar
  fronteiras, mas permitem simplificação algébrica por monotonicidade. Os ganhos
  nesses controles não demonstram por si só vantagem numa aplicação real.
- Reprodutibilidade numérica foi demonstrada no mesmo ambiente registrado,
  não entre arquiteturas diferentes ou versões arbitrárias de bibliotecas.

## 8. Próximo passo

Priorizar um circuito de decisão para **a mesma relação de lookup validada** e
para nosso GL, com vínculo de entrada e composição iguais. Só então medir custo
homogêneo para responder se a adaptação compensa. Em paralelo, fechar um contrato
global específico para a relação modular Sridhar exigirá analisar ranges e todos
os intermediários; não reutilizar o bound da nossa relação racional.

## 9. Continuidade para outra IA

1. Leia este registro e `docs/literature-comparison-protocol.md`.
2. Use dados v2 e gráficos/PDF v3; preserve versões anteriores e arquivos do usuário.
3. Reexecute o verificador antes de alterar uma conclusão.
4. Nunca rotule a tabela NFGen como execução MPC nem a tabela ZKML como prova Halo2.
5. Separe ganho do contrato, ganho de cobertura adaptativa e eventual ganho de
   eficiência. O baseline lookup não deve ser removido por superar F16 aqui.

# 0030 — Grupo B: primeiro piloto de contratos composicionais

## 1. Objetivo e estado

**Piloto off-circuit concluído; Grupo B completo ainda pendente.** O Grupo A
foi commitado antes desta etapa: `4fbf315` (`Add audited Group A figures and
thesis comparison roadmap`). Não foi feito push. `.DS_Store` e o PDF preliminar
do Grupo A ficaram fora do commit. As alterações novas do Grupo B são aditivas.

Implementamos o primeiro caso testável das figuras 5–7 de 0028: a mesma
representação fixed-point com Taylor, Padé e GL, composição pela média de
quatro sigmoids, e witnesses alternativos que satisfazem a relação completa.
O piloto verifica o caminho experimental, não a hipótese de menor custo.

## 2. Arquivos e artefatos

- `experiments/qualification_group_b_pilot_v1.json`: matriz congelada antes da execução.
- `docs/group-b-pilot-protocol.md`: distribuição, semântica, busca e limites.
- `crates/experiment-runner/src/bin/group_b_eval.rs`: geração, referências,
  exportação, replay das políticas e verificação em processo separado.
- `crates/experiment-runner/Cargo.toml`: novo binário `group-b-eval`.
- `scripts/qualification_group_b.py`: auditor independente com `Fraction`,
  tabelas derivadas e figuras Gnuplot. Reutiliza apenas a apresentação do Grupo A.
- `scripts/test_qualification_group_b.py`, `scripts/test_group_b_artifacts.py`:
  testes unitários, adulteração, preservação e reprodução.
- `scripts/run_group_b_pilot.sh`: sequência de geração, verificação e figuras.
- README e índice de progresso atualizados. Nenhum hash ou circuito legado alterado.

Resultados locais, regeneráveis e excluídos do Git pelo padrão histórico:

- `results/qualification_group_b_pilot_v1/`: dados originais (~19 MB).
- `results/qualification_group_b_pilot_replay_v1/`: segunda execução numérica.
- `results/qualification_group_b_figures_v3/`: três figuras PDF/SVG, scripts
  `.gp`, dados `.dat`, três tabelas CSV, auditoria e manifesto.
- `output/pdf/qualification_group_b_pilot_v3.pdf`: PDF consolidado, três páginas.
  Versões v1/v2 preliminares preservadas; **usar v3**.

## 3. Decisões e hipóteses

O [protocolo do piloto](../group-b-pilot-protocol.md) especifica os detalhes.
São 8 vetores aleatórios (2 seeds × 4), 5 vetores adversariais separados e
17 pontos de malha escalar. A união contém 51 entradas escalares distintas.
Cada entrada usa as 16 combinações F `{16,32,48,64}` × ordem GL `{2,4,8,16}`.
Taylor usa graus `{3,5,7,9,13}` e Padé ordens `{2,3,4,5,8}`. Todos os formatos
têm 8 bits inteiros, F fracionários e um sinal; não são chamados indiscriminadamente Q16.16.

A relação é a do checker auditado em 0026, não a antiga simulação sem bound
total de 0025. Quantização de entrada, momentos dos nós/pesos efetivamente
quantizados, cauda geométrica e slack entram no bound. O checker verifica
cada certificado com aritmética racional. Não aceita uma etiqueta `Analytical`
como evidência suficiente. O auditor Python reimplementa a derivação e as
relações sem chamar o gerador ou usar MPFR para autorizar o raio.

O verificador Rust também regenera as constantes GL para conferir sua identidade.
Isso é separado da checagem matemática: uma regra não-Legendre pode ter um
bound conservador válido, mas não pode se passar pelo benchmark GL congelado.

A média das quatro saídas usa soma/divisão racionais exatas e pode precisar
de F+2 bits fracionários. Não se omite uma requantização pretendida: ela não
faz parte desta relação. Referências MPFR dirigidas refinam de 512 até 16384
bits; não usam o valor central para determinar o predicado.

A busca em `x∈{-3/4,0,3/4}`, F16, ordem 4, visita ±32 ULP ao redor da saída
honesta e recalcula os termos. Cada candidato precisa satisfazer integralmente
o checker. A propagação posterior `v←2v` é exata e explícita; não é uma rede
ZIP nem uma cadeia de ativações sigmoid. A busca não é exaustiva sobre todos
os witnesses intermediários.

**Atribuição:** relações implícitas e a distinção entre quantização e slack
são motivadas por [Sridhar et al.](https://www.usenix.org/conference/usenixsecurity26/presentation/sridhar).
A composição, o certificado racional, as escolhas de slack, as políticas,
a decisão e este desenho de busca são extensões do repositório. Taylor/Padé
são baselines locais, não reproduções integrais de sistemas publicados. As
fontes e o mapa de comparação permanecem em [0028](0028-comparacoes-literatura-e-graficos.md).

## 4. Comandos de reprodução

Executar da raiz, usando diretório ainda inexistente. Requer Rust, Gnuplot,
Python 3 e `pypdf`. Dados brutos não são substituídos por gráficos.

```bash
bash scripts/run_group_b_pilot.sh results/qualification_group_b-NOVO
```

Para usar um Python específico: `GROUP_B_PYTHON=/caminho/python3`.
Os comandos que compõem o fluxo, executados nesta entrega:

```bash
cargo run --release --locked -p experiment-runner --bin group-b-eval -- generate \
  --matrix experiments/qualification_group_b_pilot_v1.json \
  --output-dir results/qualification_group_b_pilot_v1

target/release/group-b-eval verify \
  --matrix experiments/qualification_group_b_pilot_v1.json \
  --run-dir results/qualification_group_b_pilot_v1

python3 scripts/qualification_group_b.py \
  --run-dir results/qualification_group_b_pilot_v1 \
  --output-dir results/qualification_group_b_figures_v3 \
  --pdf output/pdf/qualification_group_b_pilot_v3.pdf

cargo test --workspace --locked --quiet
python3 -m unittest discover -s scripts -p 'test_qualification_group_*.py' -q
GROUP_B_RUN=results/qualification_group_b_pilot_v1 \
GROUP_B_REPLAY=results/qualification_group_b_pilot_replay_v1 \
  python3 -m unittest discover -s scripts -p 'test_group_b_artifacts.py' -v
```

A segunda geração usa o mesmo comando `generate`, mudando somente o diretório.
O wrapper teve sintaxe verificada; seus componentes foram executados separadamente.
Não é preciso gerar provas para validar estes certificados **numéricos**.

## 5. Testes, métricas e evidências

- 816 certificados escalares, 624 registros de decisão composta, 680 execuções
  Taylor/Padé e 1.092 trajetórias lógicas de seleção.
- 23 witnesses aceitos na busca de 195 candidatos: 7 em -3/4, 9 em zero, 7 em +3/4.
  Incluem os três witnesses honestos. Nenhum candidato rejeitado entra no máximo encontrado.
- Zero decisões certificadas contrárias à referência conclusiva.
- 16 registros de referência inconclusiva: **um mesmo vetor simétrico** no
  limiar 1/2, repetido nas 16 configurações. Não são 16 entradas independentes.
- Nos 8 vetores aleatórios, todas as 384 oportunidades de configuração/limiar
  ficaram certificadas. Esse conjunto pequeno não demonstra cobertura populacional.
- Nos adversariais, 204 decisões certificadas concordantes, 20 abstenções com
  referência conclusiva e 16 referências inconclusivas.
- Os 172 testes Rust anteriores e os 4 novos passam (176 no workspace).
  Os 30 testes Python do Grupo A e 8 unitários do Grupo B passam (38).
  Os 8 testes adicionais de artefatos também passam e verificam adulterações após recalcular o hash do arquivo,
  para exigir mais que uma simples conferência de checksum.
- Os oito arquivos numéricos `matrix`, `inputs`, `contracts`, `witnesses`,
  `decisions`, `policies`, `primitives` e `search` reproduzem byte a byte.
  Tempos, proveniência e manifestos não precisam ser idênticos.
- Três páginas finais inspecionadas visualmente, escalas comuns entre painéis
  comparáveis e legendas sem sobreposição. PDF e SVG vêm dos mesmos `.dat`.

Hashes SHA-256:

- Matriz: `432569f8cc0a11d2f746cfbb6ce0fa6e326d561ff2bdaae93dac09f7140182a3`.
- PDF v3: `5ec5712c77643c8ad279e8335a83a75c902a5490cf0150d11546d1832aad3678`.
- Todos os arquivos da execução possuem hashes em `manifest.json`; fontes,
  commit, estado dirty, diff, Cargo.lock, toolchain e SO estão em `provenance.json`.
  A proveniência é capturada no início de cada execução, não reescrita depois.

### O que os gráficos permitem explicar

**Figura 5:** em `x=0`, a saída honesta é exatamente 1/2 e tem erro zero. Ainda
assim, a relação aceita saídas que distam `4/65536 ≈ 6,10×10^-5`. O raio validado
é `7/65536 ≈ 1,07×10^-4`. Após oito multiplicações exatas por dois, o maior
desvio encontrado é `0,015625` e o bound é `0,02734375`. Isso exemplifica por
que medir só a execução honesta não caracteriza toda a liberdade da relação.
Não é um ataque a um backend publicado.

**Figura 6:** em F16, GL de ordens 4, 8 e 16 tem o mesmo máximo de erro observado
na malha (~`7,55×10^-6`), apesar de usar mais nós. Com bits fixos, aumentar a
ordem pode atingir um piso de quantização. Padé apresenta bons resultados
observados: a figura não sustenta superioridade universal de GL. Os graus e
ordens no eixo x não são uma unidade comparável de custo.

**Figura 7:** perto da fronteira 1/2, ordens GL 2 e 4 permanecem ambíguas para
os dois vetores deslocados, inclusive em F64. Ordem 8 com F32 separa ambos.
Na população adversarial desse painel, a cobertura passa de 2/4 a 4/4 referências
conclusivas; o quinto vetor continua inconclusivo na referência e é explicitamente excluído.
O aumento necessário não é determinado somente pela quantidade de bits.

**Seleção:** para `near-boundary-minus`, ordem 8, limiar 1/2, a escada visita
F16→F32 e decide False. A heurística de margem visita F16→F64, também decide
False, mas escolhe mais bits. Preview-all visita os quatro e escolhe F32.
Este resultado negativo parcial da heurística fica registrado, sem reafiná-la
depois de observar o caso. As políticas usam só enclosures, nunca a referência.

## 6. Tentativas e ajustes

- A primeira figura usava autoscale distinto por painel. V2 passou a usar
  escalas comuns; V3 deslocou a legenda da figura 6 para não cobrir os dados.
  PDFs preliminares foram preservados.
- Um teste inicialmente esperava o texto `claimed radius`, mas o checker
  corretamente rejeitou a adulteração com `understated radius`. Corrigimos
  a asserção do diagnóstico, não a relação ou a condição de aceitação.
- Fortalecida a identidade dos nós GL na verificação independente. A alteração
  não muda saídas, fórmulas, hashes de contratos ou resultados numéricos anteriores.

## 7. Limitações conhecidas

- **Zero provas ZK novas.** Certificados são objetos numéricos públicos checáveis.
  Não há compromisso criptográfico de entradas nessa bateria, circuito sigmoid
  nem adaptação heterogênea Nova. Constraints e tempos de prova estão ausentes.
- A figura 5 é um controle de amplificação linear, não a comparação completa
  de relações ZIP, erro relativo e GL em uma rede. Pior caso encontrado não é máximo universal.
- Taylor/Padé de sigmoid continuam com `ObservedGrid`, sem contrato global completo.
  Falta ponto fixo com certificação equivalente e baseline otimizado lookup/piecewise.
- A figura 6 não é uma curva de eficiência. As contagens de Horner são
  exportadas, mas custo/solver GL, largura e profundidade ainda precisam de
  instrumentação comparável. Não reutilizamos profundidades declaradas no
  simulador antigo como medidas de circuito.
- Política adaptativa é replay sobre enclosures já calculados. Não se mede
  tempo de cache como tempo de seleção real. Timings do gerador são diagnósticos,
  incluem checks internos e preparação explícita; não são 30 rodadas isoladas.
- Dois seeds com quatro vetores cada são um teste integrado, não a campanha
  estatística de 0026. Referência simétrica inconclusiva é conservadora, não
  falha matemática da identidade `sigmoid(x)+sigmoid(-x)=1`.
- Não reivindicar novidade da quadratura, intervalos ou precisão adaptativa.
  A garantia universal depende da análise do checker e correspondência à
  relação; zero contradições observadas é somente validação complementar.

## 8. Próximo passo

Completar os comparadores antes de ampliar a amostra: introduzir uma relação
lookup/piecewise com contrato verificável, manter Taylor/Padé honestos e
certificados separados e medir largura/profundidade com semânticas equivalentes.
Em seguida, ampliar as relações admissíveis da figura 5 e congelar a campanha
maior. A heurística de margem precisa considerar o piso de aproximação; qualquer
política nova terá versão e calibração separadas deste piloto.

## 9. Continuidade para outra IA

Leia 0028, este registro, `docs/group-b-pilot-protocol.md` e o contrato de 0026.
Reexecute a verificação e os testes antes de mudar a semântica. Preserve Grupo A,
v1/v2 históricos e os dados negativos do piloto. Não chamar esta entrega de
Grupo B completo ou benchmark ZK. Não ajustar inputs/ordens para embelezar a
cobertura. Novas campanhas, matrizes e resultados precisam de diretórios e
versões próprios. Não commitar `.DS_Store` ou PDFs preliminares por acidente.

# Comparação com componentes da literatura — protocolo v1

## Pergunta e unidade de comparação

Esta campanha compara **componentes numéricos efetivamente executados de
artefatos acadêmicos**, não os sistemas criptográficos completos e não os tempos
publicados. O objetivo é separar qualidade da saída honesta, liberdade da relação
aceita e suficiência de um contrato para decidir um predicado.

As referências primárias são:

- Sridhar et al., *Efficiently Provable Approximations for Non-Polynomial
  Functions*, USENIX Security 2026. [Paper](https://www.usenix.org/conference/usenixsecurity26/presentation/sridhar),
  [arquivo final-art](https://doi.org/10.5281/zenodo.21706296).
- Chen et al., *ZKML: An Optimizing System for ML Inference in Zero-Knowledge
  Proofs*, EuroSys 2024. [Paper](https://ddkang.github.io/papers/2024/zkml-eurosys.pdf),
  [repositório](https://github.com/uiuc-kang-lab/zkml).
- Fan et al., *NFGen: Automatic Non-linear Function Evaluation Code Generator
  for General-purpose MPC Platforms*, CCS 2022.
  [Paper](https://doi.org/10.1145/3548606.3560565),
  [repositório](https://github.com/Fannxy/NFGen). **MPC, não ZK.**

ZIP/CCS 2025 permanece documental: o componente desta entrega é sigmoid; não
renomeamos uma adaptação como reprodução de ZIP/GeLU. A ausência de um bound
global implementado aqui não é uma afirmação de ausência de garantias no paper.

## Versões, configurações e adaptações

`experiments/literature_sources_v1.json` fixa revisões completas. O runner confere
os arquivos consumidos contra os objetos Git, além do HEAD. Os snapshots de
ZKML e NFGen **não são declarados commits contemporâneos à publicação**.

| Família | Executado sem alterar o componente | Adaptação nossa / alcance |
|---|---|---|
| Sridhar GL | `gl_gen.py`, 4 nós, F16; ramo sigmoid de `structs.nr` em Noir | Substituir stream aleatório por entradas congeladas; entrypoint isolado. O gerador quantiza sigmoid de alta precisão, não resolve nossa relação GL. |
| Sridhar Poly | `poly_gen.py`, grau 5, F16; `check_poly_approx` | Mesmas entradas e wrapper, sem adicionar restrições à relação original. |
| Sridhar Padé | `pade_gen.py`, [3/3], F16; `check_pade_approx` | Idem; não é nossa avaliação sequencial de Padé. |
| Sridhar PWL | `pwl_gen.py`, 8 segmentos em [0,1], F16; `check_pwl_approx` | Idem. Todos os quatro geradores usam a função de alta precisão para a saída honesta. |
| ZKML lookup | Corpo Rust original de `LogisticGadgetChip::generate_map`, compilado com rustc | Configuração F16, domínio da tabela [-1,1]; tabela da campanha usa seu subconjunto [0,1]. Relação local de pertencimento à tabela, **Halo2 não executado**. |
| NFGen | `sampled_poly_fit`/Cheby e `piece_prediction` oficiais | Domínio [0,1], parâmetros n=32/f=16; graus candidatos 3/5/7/9, tolerância de calibração 1e-3. Selecionar mínimo de coeficientes entre candidatos viáveis é **nossa seleção**, não o profiler MPC. |
| Nosso GL8 | Contrato racional v1 existente; checker Rust e checker Python | Gerador de raiz implícita em mpmath, verificado exatamente; F16/32/48/64. Não é reprodução de Sridhar. |

NFGen simula arredondamento decimal, underflow e saturação com NumPy. Seu
parâmetro `f=16` **não o transforma em Q16.16 bit-exato**. As saídas binary64 do
emulador são preservadas como racionais exatos. Não executamos MP-SPDZ. A seleção
de candidatos utiliza somente a malha de calibração do gerador, não as sementes
da avaliação. O modelo escolhido e todos os candidatos são exportados.

A tabela ZKML nativa é produzida com 131.073 entradas, incluindo a linha inicial
reservada em x=-1. A avaliação e o certificado consideram somente as 65.537
entradas de [0,1]; nenhum input da campanha atinge a linha reservada. O gráfico
de constantes contabiliza explicitamente **o subconjunto usado**, não memória
alocada pelo HashMap ou custo real do circuito.

## Entradas e referência

- Malha escalar de 1.025 pontos em [0,1].
- 1.000 vetores de quatro coordenadas: 100 por seed, 20260908–20260917.
- SplitMix64 v1, coordenadas exatas `j/2^64`, incluindo zero se sorteado.
- 14 vetores adversariais separados: extremos e pontos em torno de
  `logit(tau)`, `tau ∈ {11/20,5/8,7/10}`, com offsets ±2^-24 e ±2^-48.
  Nesses vetores as quatro coordenadas são iguais. São controles de fronteira;
  admitem simplificação por monotonicidade e não demonstram, isoladamente,
  necessidade de calcular quatro ativações. Nos vetores aleatórios as quatro
  coordenadas são sorteadas separadamente.
- A média é soma/divisão **racional exata**, sem arredondamento adicional oculto.
  Não equivale automaticamente a uma camada de acumulação fixed-point de outro sistema.
- Referência dirigida MPFR256 confirmada por MPFR512 para os inputs da campanha.
  Decisões inconclusivas refinam até 16.384 bits; inconclusivas remanescentes não
  contam como acertos. A tabela exaustiva usa MPFR256 dirigido.

Os três predicados do mesmo vetor são correlacionados: 3.000 predicados não
representam 3.000 vetores independentes. O piloto não fornece intervalos de
confiança de desempenho nem generalização estatística para outras distribuições.

## Contratos e o que significa “certificado”

### Lookup e emulador congelado

Para `S=2^16`, a tabela pública T possui uma saída determinada para **cada**
`j=0..S`. Validamos todos esses pontos, não uma subamostra. Com enclosures MPFR
`[l_j,u_j]` da sigmoid em `j/S`, calculamos racionalmente:

```
e_grid = max_j max(|T_j-l_j|, |T_j-u_j|)
B = e_grid + 1/(8*S)
```

Sigmoid é globalmente 1/4-Lipschitz e RNE tem erro de entrada ≤1/(2S). Portanto,
`|T_RNE(S*x) - sigmoid(x)| ≤ B` para todo x em [0,1]. Este é um bound por
**enumeração exaustiva do domínio de entrada quantizado**, não um máximo em malha
promovido a bound. A extensão ao domínio contínuo usa a desigualdade Lipschitz.

Para ZKML, a relação local exige exatamente a saída da tabela autorizada. Para
NFGen, a garantia aplica-se **somente à tabela congelada de seu emulador Python**,
avaliado nas entradas diádicas, e não a qualquer execução ou witness MPC. As
tabelas, referências e bounds são exportados; o verificador refaz as referências
e executa novamente os componentes oficiais.

### Nosso GL

Preservamos integralmente o contrato e a derivação em
[`cdad-compositional-contract.md`](cdad-compositional-contract.md). O gerador
mpmath apenas propõe witnesses. Os checkers racionais Python e Rust validam cada
um, independentemente. `check_batch` amortiza o cálculo das constantes públicas,
mas não omite nenhum teste do checker individual nem aceita bounds externos.
Os hashes e esquemas históricos não mudam.

### Relações nativas de Sridhar

Os ramos Noir são executados para todos os witnesses honestos aceitos, controles
negativos e os 20 witnesses extremos encontrados na busca. A tradução local
preserva aritmética modular, limites estritos, índices e intermediários; ela
**não acrescenta range checks que o componente não possui**.

Não aplicamos nosso bound GL ao circuito de Sridhar: sua relação usa outros
intermediários e tolerâncias. O campo de bound global permanece `null` porque
uma derivação válida para **todos** os witnesses dessa relação modular ainda
não foi estabelecida nesta entrega. Um teste registra, por exemplo, que o ramo
polinomial não fixa `intermediates[0]` a SCALE. Isso é uma observação delimitada
do componente fixado, não uma conclusão sobre vulnerabilidade do sistema inteiro.

A busca varia y em ±256 ULP nas entradas 0,1/4,1/2,3/4,1. Em GL reconstrói os
intermediários; nos demais métodos conserva os intermediários honestos. Registra
o maior desvio encontrado. Não é busca exaustiva sobre todos os witnesses.

### Decisão e adaptação

Para `[l,u]` e `y>=tau`: verdadeiro se `l>=tau`, falso se `u<tau`, abstenção no
restante. Média de quatro sigmoids: somar quatro intervalos e dividir por quatro;
nenhuma hipótese de independência dos erros.

As políticas fixa16, fixa64, sequencial, margem e preview-all usam os mesmos
perfis GL8. A seleção por margem consulta somente saída e enclosure. O custo
exportado é **número de avaliações por replay**, não tempo de uma política real,
nem prova de que o histórico de tentativas foi executado. Tempos do harness
incluem preparação/checagem heterogêneas e não sustentam ranking entre métodos.

## Reprodução

Pré-requisitos: Rust/Cargo, Git, Python 3.14, Gnuplot; dependências Python são
instaladas isoladamente em `target/literature-venv`. O wrapper instala Noir
beta.21 em cache no macOS ARM64, verificando SHA256 do arquivo. Em outra plataforma,
o binário correspondente deve ser fornecido no caminho documentado.

```sh
bash scripts/run_literature_comparison.sh results/qualification_literature_NOVO
```

O comando prepara fontes fixadas, testa componentes, gera dados, executa o
verificador em **outro processo** e produz PDF/SVG. Recusa diretórios existentes.

Verificar uma campanha já existente, sem gerar gráficos ou substituir dados:

```sh
target/literature-venv/bin/python scripts/literature_compare.py verify \
  --run-dir results/qualification_literature_v2
```

São preservados `raw.jsonl`, `raw.csv`, inputs exatos, witnesses, tabelas completas,
referências dirigidas, logs de validação Noir, perfis e proveniência. A declaração
`complete-off-circuit` só aparece depois de concluir a geração; uma falha deixa
`failure.json` e nenhum manifesto completo. O verificador não confia apenas nos
hashes: regenera fontes numéricas, referências, bounds, decisões e seleção.

## Figuras e restrições de interpretação

1. Erro honesto: os quatro geradores Sridhar coincidem porque partem da mesma
   função de alta precisão. Não interpretar como igualdade dos quatro algoritmos.
2. Saídas admissíveis: maior desvio encontrado não é máximo universal. Bounds
   de outro painel têm escopos distintos, explicitados acima.
3. Decisões compostas: azul é concordância observada, não “artigo sem garantia”.
4. Mesmo método sem/com contrato: os valores não mudam; muda a decisão de emitir
   ou abster-se. Isola o ganho semântico do contrato.
5. Adaptação: separar aleatórios e adversariais; não converter avaliações em
   tempo ou constraints. Mais bits podem não resolver o piso de erro GL8.
6. Constantes versus qualidade: não é medição de memória ou fronteira de proving.

Nenhuma demonstração de novidade científica resulta só de atribuir um novo nome
ao contrato. A comparação acadêmica deve destacar o que foi reproduzido e o
que foi acrescentado, inclusive quando o baseline lookup for melhor.

# proof_numeric

Experimento reproduzível sobre representações fracionárias em limbs para
computação iterativa verificável.

## Pacote de qualificação

O [relatório para o orientador](docs/qualification-results.md) sintetiza a evidência e seus limites.
O [guia de qualificação](docs/qualification.md) reúne a matriz dinâmica,
provas comprimidas verificáveis em processo separado e gráficos Gnuplot PDF/SVG.
O [Grupo A auditado](docs/progress/0029-grupo-a-figuras-auditadas.md) acrescenta
as figuras 1–4 do plano bibliográfico: trajetórias, resultados de decisão,
cobertura por regime e margem versus incerteza, sem reexecutar a dinâmica.
Para uma entrega nova, sem sobrescrever resultados:

```bash
bash scripts/run_qualification.sh results/qualificacao-NOVO quick
```

Troque `quick` por `full` para a matriz completa e 30 rodadas no cenário de
timing predefinido. A contribuição avaliada é a preservação justificável da
decisão, com cobertura e custo explícitos. As entradas são públicas; o pacote
não reivindica privacidade nem Nova heterogêneo. Experimentos antigos permanecem
disponíveis, mas os scripts históricos de timing não devem ser usados para
esta campanha, pois reutilizam diretórios de saída.

Princípio central: **representation alone does not determine semantic
fidelity**. Para os novos experimentos, a semântica numérica é a composição
explícita de representação, operações, aproximação e contrato de erro. A
validade criptográfica continua separada da fidelidade numérica e da decisão da
aplicação.

O projeto separa deliberadamente:

1. referência matemática exata;
2. aritmética finita fora do circuito;
3. aritmetização R1CS;
4. IVC e compressão com Nova.

## Comandos

```bash
cargo test --workspace
cargo bench --workspace
cargo run --release -p experiment-runner -- --help
cargo run --release -p nova-circuit -- --help
cargo run --release -p nova-circuit -- --steps 10 --k 2 \
  --rounding nearest-even --representation fp32 --compress

# Funções não polinomiais; o diretório de saída deve ser novo
bash scripts/run_non_polynomial_experiments.sh \
  results/non_polynomial_run_20260901
```

Dependências são fixadas em `Cargo.lock`. A integração criptográfica usa
`nova-snark = "=0.73.0"` (commit de publicação
`666e3b25bfb9f8b2106f8b4d8057010f28b1ee79`).

Os resultados medidos em máquinas ou backends diferentes não são tratados como
benchmarks homogêneos.

## Artefatos

- [Plano de execução da tese](docs/thesis-execution-plan.md)
- [Diário de progresso e ponto de retomada](docs/progress/README.md)
- [Último avanço: figuras auditadas do Grupo A](docs/progress/0029-grupo-a-figuras-auditadas.md)
- [Especificação CFD](docs/cfd-mathematical-specification.md)
- [Especificação CDAD](docs/cdad-mathematical-specification.md)
- [Protocolo experimental CDAD](docs/cdad-protocol.md)
- [Semântica numérica e funções não polinomiais](docs/numeric-semantics-non-polynomial.md)
- [Relatório](docs/report.md)
- [Especificação matemática](docs/mathematical-specification.md)
- [Bounds no corpo finito](docs/finite-field-bounds.md)
- [Modelo local FP32](docs/ieee754-cost-model.md)
- [Especificação do baseline FP32 R1CS](docs/fp32-r1cs.md)
- [Reprodutibilidade](docs/reproducibility.md)
- `data/`: trajetórias e resumos brutos;
- `results/`: tabelas, métricas Nova e gráficos consolidados.

Estado atual: a implementação off-circuit cobre todos os formatos planejados.
A step circuit e o IVC medidos cobrem `P=16, K=1..4` com truncagem, há smoke
test/medição curta para `nearest-even` em `K=2`, há uma variante `K=1..4`
limb-a-limb com carries explícitos para o produto `N(S-N)`, e há uma primeira
step circuit compensada para `K=2` com estado `V,E` em grade `S²`. O circuito
compensado ainda é inicial: cobre truncagem e `nearest-even` na grade fina,
mas ainda não cobre outros valores de `K`.

A CBD v1 também está implementada:

- referência racional com intervalos, operações fusionadas e decisões
  trivaloradas;
- bounds global e local para o mapa logístico;
- step circuit Nova `K=1..4` com bound local, nearest-even e predicado
  `x>=0,5` constrained;
- ablação sequencial `K=1..4`, com quantização após o produto e após `r`;
- IVC e compressão CBD;
- avaliações no mapa logístico, UCI Adult e proximidade ECEF.

O baseline FP32 positivo-normal possui referência de bits, gadgets R1CS exatos
para multiplicação, adição e `1-x`, etapas logística e afim com decisão, IVC e
compressão.
No mesmo backend, a etapa FP32 medida custa 1.721 constraints contra 709 do
CBD-32 fusionado e 782 do CBD-32 sequencial. A vantagem bruta permanece quando
a ordem operacional é igualada, embora CBD não implemente IEEE 754 geral.

A CFD v1 acrescenta significando IEEE-shaped, corredor público de expoentes,
raio em ULPs, fallback `wide` e decisões trivaloradas propagadas entre folds.
Na topologia sequencial, a etapa logística custa 1.003 constraints e a etapa
afim 846, contra 1.721 e 1.418 dos respectivos FP32 exatos. A avaliação até
1.000 passos não encontrou decisões definitivas incorretas, mas expôs perda
rápida de cobertura nos regimes caóticos; por isso o resultado científico é
registrado como go parcial, não como dominância geral.

A CDAD v1 acrescenta uma escada pública de precisão e três políticas de
seleção (`fixed`, `sequential` e `margin-directed`). O controlador pode saltar
provas intermediárias após pré-avaliações certificadas locais, mas toda decisão
continua vindo do enclosure CBD. A matriz off-circuit cobre os quatro workloads;
o protótipo Nova fixo cobre 16/32/48/64 bits e contabiliza separadamente cada
tentativa e vínculo. Um gadget homogêneo de produto interno assinado cobre os
scores de Adult e ECEF nas quatro precisões, com range checks, nearest-even,
propagação conservadora de raio e decisão trivalorada. A adaptação heterogênea
dentro de uma única instância Nova e o compromisso vetorial especializado
permanecem fora da v1.

## Funções não polinomiais e influência de Sridhar et al.

O grupo **primitive/function fidelity** complementa os workloads iterativos sem
substituí-los. Ele contém `exp` e `sigmoid`, referências MPFR dirigidas,
fixed-point assinado e aproximações Taylor, Padé diagonal e Gauss-Legendre
implícita. A matriz pública cobre Q8.8 até Q120.120, as duas semânticas de
arredondamento encontradas no paper e no artefato, ordens configuráveis, erro
decomposto, margens de decisão e contagens de operações.

Sridhar et al. motivam diretamente a separação entre quantização honesta e
slack de verificação, Padé, Gauss-Legendre com soma ponderada e a distinção
entre largura e profundidade. Numeric profiles, taxonomia L1/L2/L3, estabilidade
orientada à decisão, precisão adaptativa e futura composição com CDAD/Nova são
extensões deste projeto, não contribuições atribuídas ao paper.

O backend do paper é Noir/Barretenberg. Esta entrega é deliberadamente
off-circuit: `estimated_constraints` e `actual_constraints` são exportados
como ausentes, nunca estimados a partir de sistemas heterogêneos.

### Contratos composicionais: marco de auditoria

O [contrato composicional](docs/cdad-compositional-contract.md) acrescenta um
checker racional independente para uma nova relação de sigmoid, propagação de
enclosures e quatro políticas de seleção com todos os previews contabilizados.
`contract-audit` exporta certificados e uma demonstração pequena de composição
sigmoid e distância quadrática. Essa relação é extensão nossa, não reprodução
do circuito de Sridhar. O runner de funções agora usa decisões MPFR dirigidas
e resultados esquema 2; o slack GL histórico não é um bound total certificado.

O [protocolo competitivo](docs/cdad-competitive-protocol.md) registra os gates
antes da campanha. Compromisso vetorial, novos circuitos e vantagem de tempo
completo **ainda não foram demonstrados**. Resultados anteriores são preservados.

O desenvolvimento da representação CBD é documentado em registros cronológicos
imutáveis. Para retomar o trabalho, leia primeiro o plano e depois o último
registro listado no diário de progresso.

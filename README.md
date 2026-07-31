# proof_numeric

Experimento reproduzível sobre representações fracionárias em limbs para
computação iterativa verificável.

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
```

Dependências são fixadas em `Cargo.lock`. A integração criptográfica usa
`nova-snark = "=0.73.0"` (commit de publicação
`666e3b25bfb9f8b2106f8b4d8057010f28b1ee79`).

Os resultados medidos em máquinas ou backends diferentes não são tratados como
benchmarks homogêneos.

## Artefatos

- [Plano de execução da tese](docs/thesis-execution-plan.md)
- [Diário de progresso e ponto de retomada](docs/progress/README.md)
- [Último avanço: Certified Floating Dyadic](docs/progress/0016-certified-floating-dyadic.md)
- [Especificação CFD](docs/cfd-mathematical-specification.md)
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

O desenvolvimento da representação CBD é documentado em registros cronológicos
imutáveis. Para retomar o trabalho, leia primeiro o plano e depois o último
registro listado no diário de progresso.

# Reprodutibilidade

- Rust: 1.85.0, fixado em `rust-toolchain.toml`;
- `nova-snark`: 0.73.0;
- commit incorporado no crate:
  `666e3b25bfb9f8b2106f8b4d8057010f28b1ee79`;
- `Cargo.lock` versiona todas as dependências, incluindo
  `serde_with=3.8.3` para respeitar o toolchain;
- CPU medida: Apple M5 Pro, 15 cores;
- memória: 24 GB;
- SO: macOS 27.0;
- perfil release: overflow checks, thin LTO e símbolos de debug;
- FMA dos baselines: desabilitado pela separação das operações e barreiras
  `black_box`;
- referência longa: MPFR round-to-nearest, com precisão automática
  `max(256, 2*N+128)`.
- pico de memória Nova: `getrusage(RUSAGE_SELF).ru_maxrss`; no macOS o valor é
  registrado em bytes, em Linux o binário converte de KiB para bytes.

Comandos:

```bash
cargo test --workspace
cargo bench --workspace
bash scripts/run_numeric_experiments.sh
REPETITIONS=3 bash scripts/run_nova_benchmarks.sh
python3 scripts/consolidate_results.py
```

CBD:

```bash
cargo run --release -p nova-circuit -- \
  --representation cbd --rounding nearest-even \
  --k 2 --steps 10 --compress \
  --output results/nova_cbd_local_decision_k2_n10_compressed.json

python3 scripts/train_adult_logistic.py \
  --input /caminho/para/adult.data \
  --output results/adult_logistic_model.json

cargo run --release -p experiment-runner --bin workload-eval -- \
  --adult-model results/adult_logistic_model.json \
  --output results/cbd_workloads.json
```

FP32 homogêneo:

```bash
cargo run --release -p nova-circuit -- \
  --representation fp32 --rounding nearest-even \
  --k 2 --steps 10 --compress \
  --output results/nova_fp32_exact_n10_compressed.json
```

Repita CBD e FP32 três vezes com nomes de saída distintos. Os resultados
consolidados desta rodada estão em `results/fp32_cbd_go_no_go.json`.

Ablação de fusão:

```bash
for r in 3.5 3.7 3.9 4.0; do
  safe_r=${r/./_}
  cargo run --release -p experiment-runner -- \
    --steps 1000 --r "$r" \
    --output "data/trajectory_ablation_r${safe_r}_n1000.csv"
done

cargo run --release -p nova-circuit -- \
  --representation cbd-sequential --rounding nearest-even \
  --k 2 --steps 10 --compress \
  --output results/nova_cbd_sequential_k2_n10_rep1.json

python3 scripts/analyze_cbd_ablation.py
```

Repita o benchmark Nova três vezes. O consolidador lê os JSONs `rep1..rep3`
e produz `results/cbd_fusion_ablation.json`.

O Adult deve ser obtido do UCI, DOI `10.24432/C5XW20`. O artefato treinado
registra SHA-256 do `adult.data`.

A validação Berkeley SoftFloat está isolada porque o wrapper não compila em
Apple Silicon:

```bash
# Em host x86_64:
cargo test -p softfloat-validation
```

Em `aarch64-apple-darwin`, a suíte principal usa `softfloat 1.0.0` como
implementação software independente e mantém o teste Berkeley para execução
posterior em x86_64.

Os scripts não fixam afinidade de CPU nem desabilitam serviços do sistema.
Para publicação, execute-os em máquina ociosa e registre frequência/energia.

## Certified Floating Dyadic

```bash
# referência racional/MPFR, 43 configurações e 1.000 passos
cargo run --release -p experiment-runner --bin cfd-eval -- \
  --steps 1000 --output results/cfd_workloads.json

# topologia sequencial comparável ao FP32
for n in 10 100 1000; do
  cargo run --release -p nova-circuit -- --steps "$n" --k 1 \
    --rounding nearest-even --representation cfd \
    --output "results/nova_cfd_sequential_p24_lanes9_n${n}.json"
done

# ablação fusionada, score afim e baseline afim FP32 exato
for representation in cfd-fused cfd-stable fp32-stable; do
  cargo run --release -p nova-circuit -- --steps 10 --k 1 \
    --rounding nearest-even --representation "$representation" --compress \
    --output "results/nova_${representation}_n10_compressed.json"
done
```

Os JSONs registram constraints brutas, circuito Nova aumentado, estado, setup,
folds, verificação, compressão, prova serializada e pico RSS. Tempos obtidos
com processos concorrentes não devem ser comparados; repita cada perfil
isoladamente e com aquecimento para uma tabela temporal publicável.

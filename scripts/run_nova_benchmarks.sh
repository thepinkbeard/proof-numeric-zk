#!/usr/bin/env bash
set -euo pipefail

repetitions="${REPETITIONS:-3}"

# Warm-up não registrado.
cargo run --release -p nova-circuit -- --steps 10 --k 2 \
  --output results/warmup.json >/dev/null

for k in 1 2 3 4; do
  cargo run --release -p nova-circuit -- --steps 10 --k "$k" --compress \
    --output "results/nova_k${k}_n10_constraints.json"
done

for steps in 10 100 1000; do
  for repetition in $(seq 1 "$repetitions"); do
    cargo run --release -p nova-circuit -- --steps "$steps" --k 2 --compress \
      --output "results/nova_k2_n${steps}_rep${repetition}.json"
  done
done

# Go/no-go homogêneo: mesma curva, decisão, folds e compressão.
for representation in cbd cbd-sequential fp32; do
  for repetition in $(seq 1 "$repetitions"); do
    cargo run --release -p nova-circuit -- --steps 10 --k 2 \
      --rounding nearest-even --representation "$representation" --compress \
      --output "results/nova_${representation}_exact_n10_rep${repetition}.json"
  done
done

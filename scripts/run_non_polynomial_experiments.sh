#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <new-output-directory>" >&2
  exit 2
fi

output_dir=$1

cargo run --release -p experiment-runner --bin function-eval -- \
  --matrix experiments/non_polynomial_v1.json \
  --output-dir "$output_dir" \
  --seed 20260901

python3 scripts/plot_non_polynomial.py \
  --input-dir "$output_dir" \
  --output-dir "$output_dir/figures"

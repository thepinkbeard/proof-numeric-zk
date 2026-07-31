#!/usr/bin/env bash
set -euo pipefail

for r in 3.5 3.7 3.9 4.0; do
  for horizon in 10 30 100 1000 10000; do
    safe_r=${r/./_}
    cargo run --release -p experiment-runner -- \
      --steps "$horizon" \
      --r "$r" \
      --output "data/trajectory_r${safe_r}_n${horizon}.csv"
  done
done


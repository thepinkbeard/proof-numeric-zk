#!/usr/bin/env bash
set -euo pipefail
if [ "$#" -ne 1 ]; then
  echo "Usage: bash scripts/run_group_b_pilot.sh <new-output-directory>" >&2
  exit 2
fi
group_b_root="$1"
group_b_python="${GROUP_B_PYTHON:-python3}"
test -f experiments/qualification_group_b_pilot_v1.json || {
  echo "Run from the repository root." >&2
  exit 2
}
mkdir "$group_b_root"
cargo build --release --locked -p experiment-runner --bin group-b-eval
target/release/group-b-eval generate \
  --matrix experiments/qualification_group_b_pilot_v1.json \
  --output-dir "$group_b_root/numeric"
target/release/group-b-eval verify \
  --matrix experiments/qualification_group_b_pilot_v1.json \
  --run-dir "$group_b_root/numeric"
"$group_b_python" scripts/qualification_group_b.py \
  --run-dir "$group_b_root/numeric" \
  --output-dir "$group_b_root/figures" \
  --pdf "$group_b_root/group-b.pdf"

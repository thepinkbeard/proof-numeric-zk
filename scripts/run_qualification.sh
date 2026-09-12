#!/usr/bin/env bash
# Immutable end-to-end package. Do not run other CPU-heavy work during timing.
set -euo pipefail
cd "$(dirname "$0")/.."
output="${1:?Usage: bash scripts/run_qualification.sh NEW_OUTPUT_DIRECTORY [quick]}"
mode="${2:-full}"
if [[ "$mode" != full && "$mode" != quick ]]; then echo "mode must be full or quick" >&2; exit 2; fi
mkdir "$output"
output="$(cd "$output" && pwd)"
command -v gnuplot >/dev/null
cargo test --workspace --locked
cargo build --release --locked -p experiment-runner --bin qualification -p nova-circuit --bin nova-circuit
target/release/qualification generate --output-dir "$output/inputs"
extra=()
if [[ "$mode" == quick ]]; then extra=(--max-inputs 2 --max-horizon 30); fi
target/release/qualification evaluate --inputs-dir "$output/inputs" --output-dir "$output/numeric" "${extra[@]}"
python3 scripts/qualification.py scenarios --output-dir "$output/statements"
# All short scenarios are proved at every fixed precision and adaptive policy.
python3 scripts/qualification.py campaign --statements "$output/statements" --output-dir "$output/demonstration" --rounds 1
QUAL_BUNDLE="$output/demonstration/boundary-2-round1-sequential-16" \
  QUAL_STATEMENT="$output/statements/boundary-2.json" \
  python3 scripts/test_qualification.py
timing="$output/demonstration"
if [[ "$mode" == full ]]; then
  # Prespecified primary timing scenario, separate from the demonstration.
  python3 scripts/qualification.py campaign --statements "$output/statements" --output-dir "$output/timing" --rounds 30 --scenario boundary-10
  timing="$output/timing"
fi
python3 scripts/analyze_qualification_timing.py --input-dir "$timing" --output "$output/timing-analysis.json"
python3 scripts/plot_qualification.py --input-dir "$output/numeric" --timing-dir "$timing" --output-dir "$output/figures"
echo "Completed $mode package in $output. Read figures/report.md; quick mode is not the full campaign."

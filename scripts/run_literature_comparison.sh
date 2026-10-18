#!/usr/bin/env bash
set -euo pipefail
if [ "$#" -ne 1 ]; then
  echo "Usage (from repository root): bash scripts/run_literature_comparison.sh <NEW-run-directory>" >&2
  exit 2
fi
literature_run="$1"
test ! -e "$literature_run" || { echo "Refusing to overwrite $literature_run" >&2; exit 2; }
test -f experiments/literature_sources_v1.json
if [ ! -x target/literature-venv/bin/python ]; then
  python3 -m venv target/literature-venv
fi
target/literature-venv/bin/pip install -r scripts/literature-requirements.txt
mkdir -p target/literature-artifacts/noir-beta21
if [ ! -x target/literature-artifacts/noir-beta21/nargo ]; then
  if [ "$(uname -sm)" != "Darwin arm64" ]; then
    echo "Install nargo 1.0.0-beta.21 for your platform at target/literature-artifacts/noir-beta21/nargo." >&2
    echo "https://github.com/noir-lang/noir/releases/tag/v1.0.0-beta.21" >&2
    exit 2
  fi
  curl -fL --retry 2 -o target/literature-artifacts/nargo-beta21.tar.gz \
    https://github.com/noir-lang/noir/releases/download/v1.0.0-beta.21/nargo-aarch64-apple-darwin.tar.gz
  target/literature-venv/bin/python -c "import hashlib; from pathlib import Path; p=Path('target/literature-artifacts/nargo-beta21.tar.gz'); assert hashlib.sha256(p.read_bytes()).hexdigest() == '78cb9cbb4c9d173ef0b22a9eaf80a00fb43382c78868ae820e375424fd6f4e71', 'nargo archive digest mismatch'"
  tar -xzf target/literature-artifacts/nargo-beta21.tar.gz -C target/literature-artifacts/noir-beta21
fi
cargo build --release --locked -p experiment-runner --bin literature-bridge
target/literature-venv/bin/python scripts/literature_compare.py prepare
target/literature-venv/bin/python -m unittest discover -s scripts -p test_literature_comparison.py
mkdir "$literature_run"
target/literature-venv/bin/python scripts/literature_compare.py generate --run-dir "$literature_run/numeric"
target/literature-venv/bin/python scripts/literature_compare.py verify --run-dir "$literature_run/numeric"
target/literature-venv/bin/python scripts/plot_literature_comparison.py \
  --run-dir "$literature_run/numeric" --output-dir "$literature_run/figures" --pdf "$literature_run/comparison.pdf"

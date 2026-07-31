#!/usr/bin/env bash
# Benchmark de tempo com repetições suficientes para intervalo de confiança.
#
# As 3 repetições de 0006 não sustentavam ordenação de tempos, e o próprio
# `docs/report.md` reconhecia isso. O problema não era só o tamanho da amostra:
# era o desenho. Rodar as N repetições de uma representação e depois as N da
# seguinte confunde representação com tempo de parede — se a máquina esquenta,
# entra em throttling ou pega uma tarefa de fundo no meio da série, o efeito cai
# inteiro sobre a representação que estava rodando naquele momento, e vira
# diferença aparente entre representações.
#
# Aqui as representações são intercaladas: cada rodada executa todas elas, em
# ordem embaralhada dentro da rodada. Deriva lenta afeta todas as
# representações quase igualmente, e o que sobra é comparável **dentro** da
# rodada. `analyze_timing.py` explora exatamente isso com comparação pareada.
#
# Uso: [ROUNDS=30] [STEPS=10] scripts/run_timing_benchmarks.sh
set -euo pipefail

cd "$(dirname "$0")/.."

rounds="${ROUNDS:-30}"
steps="${STEPS:-10}"
output_dir="results/timing"
binary="${CARGO_TARGET_DIR:-target}/release/nova-circuit"

representations=(cbd cbd-sequential cfd fp32 standard)

cargo build --release -p nova-circuit
mkdir -p "$output_dir"
rm -f "$output_dir"/*.json

run_one() {
  local representation="$1" round="$2"
  local args=(--steps "$steps" --rounding nearest-even --compress
              --representation "$representation")
  # `cfd` exige K=1; as demais rodam em K=2. Fora isso a configuração é a mesma
  # para todas, que é o que torna a comparação homogênea.
  if [[ "$representation" == "cfd" ]]; then
    args+=(--k 1 --precision 24 "--exponent-lanes=-4:0")
  else
    args+=(--k 2)
  fi
  "$binary" "${args[@]}" \
    --output "$output_dir/${representation}_round${round}.json" >/dev/null
}

echo "== estado da máquina antes =="
uptime

# Rodada de aquecimento, descartada: a primeira execução paga cache frio de
# página e de instruções, e entraria na amostra como um outlier sistemático.
echo "== aquecimento =="
for representation in "${representations[@]}"; do
  run_one "$representation" 0
done
rm -f "$output_dir"/*_round0.json

for round in $(seq 1 "$rounds"); do
  # Ordem embaralhada a cada rodada: se uma representação ficasse sempre em
  # primeiro lugar, herdaria sistematicamente o estado de cache deixado pela
  # última da rodada anterior.
  shuffled=$(printf '%s\n' "${representations[@]}" | sort -R)
  while read -r representation; do
    run_one "$representation" "$round"
  done <<< "$shuffled"
  printf 'rodada %d/%d\n' "$round" "$rounds"
done

echo "== estado da máquina depois =="
uptime

echo "$rounds rodadas x ${#representations[@]} representações em $output_dir"

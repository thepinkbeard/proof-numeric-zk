#!/usr/bin/env bash
# Varredura de custo da fronteira de Pareto.
#
# Emite um JSON por configuração em results/nova_sweep/, cada um contendo o
# profile_hash_hex que junta com results/frontier_coverage.json.
#
# A matriz precisa espelhar a de crates/experiment-runner/src/bin/frontier_eval.rs.
# Uma configuração medida aqui e ausente lá (ou o contrário) não vira erro: ela
# simplesmente some da fronteira na junção. Por isso build_pareto.py reporta as
# duas listas de órfãos.
#
# --steps é deliberadamente pequeno: raw_step_constraints mede uma etapa e não
# depende do número de etapas. Rodar 1.000 passos aqui multiplicaria o tempo de
# prova sem mudar o eixo de custo.

set -euo pipefail

STEPS="${STEPS:-2}"
OUTPUT_DIR="${OUTPUT_DIR:-results/nova_sweep}"
BINARY="${BINARY:-}"

if [[ -z "$BINARY" ]]; then
  cargo build --release -p nova-circuit
  BINARY="${CARGO_TARGET_DIR:-target}/release/nova-circuit"
fi

mkdir -p "$OUTPUT_DIR"

total=0
failed=0

emit() {
  local name="$1"
  shift
  total=$((total + 1))
  if "$BINARY" --steps "$STEPS" --output "$OUTPUT_DIR/$name.json" "$@" >/dev/null 2>"$OUTPUT_DIR/$name.err"; then
    rm -f "$OUTPUT_DIR/$name.err"
    printf '  ok   %s\n' "$name"
  else
    failed=$((failed + 1))
    printf '  FALHA %s: %s\n' "$name" "$(tail -n 1 "$OUTPUT_DIR/$name.err")"
  fi
}

echo "== mapa logístico =="
# r e x0 fixos: o custo de circuito não depende deles, mas eles entram no perfil
# e portanto no hash. Precisam coincidir com os da cobertura ou a junção falha.
for r in 7/2 37/10 39/10 4; do
  tag="r$(echo "$r" | tr '/' '_')"
  for threshold in 1/4 1/2 3/4; do
    th="t$(echo "$threshold" | tr '/' '_')"
    common=(--workload logistic-map --r "$r" --x0 1/10 --threshold "$threshold")

    for k in 1 2 3 4; do
      precision=$((16 * k))
      for rounding in truncate nearest-even; do
        emit "standard_k${k}_${rounding}_${tag}_${th}" \
          --representation standard --k "$k" --precision "$precision" \
          --rounding "$rounding" "${common[@]}"
        emit "carry_k${k}_${rounding}_${tag}_${th}" \
          --representation carry --k "$k" --precision "$precision" \
          --rounding "$rounding" "${common[@]}"
      done
      emit "cbd_global_k${k}_${tag}_${th}" \
        --representation cbd-global --k "$k" --precision "$precision" \
        --rounding nearest-even "${common[@]}"
      emit "cbd_local_k${k}_${tag}_${th}" \
        --representation cbd --k "$k" --precision "$precision" \
        --rounding nearest-even "${common[@]}"
      emit "cbd_sequential_k${k}_${tag}_${th}" \
        --representation cbd-sequential --k "$k" --precision "$precision" \
        --rounding nearest-even "${common[@]}"
    done

    for rounding in truncate nearest-even; do
      emit "compensated_${rounding}_${tag}_${th}" \
        --representation compensated --k 2 --precision 32 \
        --rounding "$rounding" "${common[@]}"
    done

    for precision in 16 24 32; do
      for minimum in -4 -8 -16; do
        lanes="${minimum}:0"
        lane_tag="e$(echo "$minimum" | tr -d '-')"
        emit "cfd_p${precision}_${lane_tag}_${tag}_${th}" \
          --representation cfd --k 1 --precision "$precision" \
          --exponent-lanes="$lanes" --radius-bits 16 \
          --rounding nearest-even "${common[@]}"
        emit "cfd_fused_p${precision}_${lane_tag}_${tag}_${th}" \
          --representation cfd-fused --k 1 --precision "$precision" \
          --exponent-lanes="$lanes" --radius-bits 16 \
          --rounding nearest-even "${common[@]}"
      done
    done

    # FP32 só decide contra limiar potência de dois: o gadget compara expoentes
    # enviesados. Limiares 1/4 e 1/2 passam, 3/4 não.
    case "$threshold" in
      1/4 | 1/2)
        emit "fp32_${tag}_${th}" \
          --representation fp32 --k 1 --precision 24 \
          --rounding nearest-even "${common[@]}"
        ;;
    esac
  done
done

echo "== recorrência afim estável =="
for pair in "1/2 3/4 diadico" "7/10 1/2 nao_diadico"; do
  read -r alpha innovation regime <<<"$pair"
  for threshold in 1 5/4 3/2; do
    th="t$(echo "$threshold" | tr '/' '_')"
    common=(--workload stable-affine --alpha "$alpha" --innovation "$innovation"
            --x0 1/10 --threshold "$threshold")

    for k in 1 2 3 4; do
      precision=$((16 * k))
      for rounding in truncate nearest-even; do
        emit "affine_standard_k${k}_${rounding}_${regime}_${th}" \
          --representation standard --k "$k" --precision "$precision" \
          --rounding "$rounding" "${common[@]}"
      done
      emit "affine_cbd_k${k}_${regime}_${th}" \
        --representation cbd --k "$k" --precision "$precision" \
        --rounding nearest-even "${common[@]}"
    done

    for precision in 16 24 32; do
      for minimum in -16 -24; do
        lane_tag="e$(echo "$minimum" | tr -d '-')"
        emit "affine_cfd_p${precision}_${lane_tag}_${regime}_${th}" \
          --representation cfd-stable --k 1 --precision "$precision" \
          --exponent-lanes="${minimum}:1" --radius-bits 16 \
          --rounding nearest-even "${common[@]}"
      done
    done

    if [[ "$threshold" == "1" ]]; then
      emit "affine_fp32_${regime}_${th}" \
        --representation fp32-stable --k 1 --precision 24 \
        --rounding nearest-even "${common[@]}"
    fi
  done
done

echo
echo "$total configurações, $failed falhas, saída em $OUTPUT_DIR"

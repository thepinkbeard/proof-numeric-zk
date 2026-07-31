#!/usr/bin/env python3
"""Consolida a ablação CBD fusionado × sequencial × FP32."""

from __future__ import annotations

import csv
import json
from collections import defaultdict
from fractions import Fraction
from pathlib import Path
from statistics import mean


ROOT = Path(__file__).resolve().parents[1]


def load_json(path: Path):
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def trajectory_metrics():
    groups = defaultdict(
        lambda: {
            "states": 0,
            "definite_025": 0,
            "definite_050": 0,
            "definite_075": 0,
            "unsound": 0,
            "width_sum": Fraction(0),
            "width_max": Fraction(0),
        }
    )
    for path in sorted((ROOT / "data").glob("trajectory_ablation_r*_n1000.csv")):
        with path.open(encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle):
                if not (
                    row["format"].endswith("_local")
                    and row["format"].startswith(("cbd_16_", "cbd_32_", "cbd_48_", "cbd_64_"))
                    and (
                        row["format"].endswith("_sequential_local")
                        or not row["format"].endswith("_lipschitz")
                    )
                ):
                    continue
                key = (row["r"], row["format"])
                group = groups[key]
                group["states"] += 1
                for suffix in ("025", "050", "075"):
                    if row[f"decision_{suffix}"] != "indeterminate":
                        group[f"definite_{suffix}"] += 1
                if any(
                    row[f"unsound_certified_{suffix}"] == "true"
                    for suffix in ("025", "050", "075")
                ):
                    group["unsound"] += 1
                width = Fraction(row["certified_upper"]) - Fraction(row["certified_lower"])
                group["width_sum"] += width
                group["width_max"] = max(group["width_max"], width)

    result = []
    for (r_value, representation), group in sorted(groups.items()):
        states = group.pop("states")
        width_sum = group.pop("width_sum")
        width_max = group.pop("width_max")
        result.append(
            {
                "r": r_value,
                "representation": representation,
                "states": states,
                **group,
                "definite_total": sum(
                    group[f"definite_{suffix}"] for suffix in ("025", "050", "075")
                ),
                "mean_interval_width": float(width_sum / states),
                "maximum_interval_width": float(width_max),
            }
        )
    return result


def profile_constraints():
    rows = []
    for k in range(1, 5):
        sequential_name = (
            f"nova_cbd_sequential_k{k}_n2_smoke.json"
            if k == 2
            else f"nova_cbd_sequential_k{k}_n2.json"
        )
        sequential = load_json(ROOT / "results" / sequential_name)
        fused = load_json(
            ROOT / "results" / f"nova_cbd_local_decision_k{k}_n2.json"
        )
        rows.append(
            {
                "fractional_bits": 16 * k,
                "fused_constraints": fused["raw_step_constraints"],
                "sequential_constraints": sequential["raw_step_constraints"],
                "sequential_over_fused": (
                    sequential["raw_step_constraints"] / fused["raw_step_constraints"]
                ),
                "fp32_constraints": 1721,
                "sequential_over_fp32": (
                    sequential["raw_step_constraints"] / 1721
                ),
            }
        )
    return rows


def timing_summary(pattern: str):
    records = [load_json(path) for path in sorted((ROOT / "results").glob(pattern))]
    return {
        "repetitions": len(records),
        "prover_total_seconds_mean": mean(
            row["prover_total_seconds"] for row in records
        ),
        "compression_seconds_mean": mean(
            row["compression_seconds"] for row in records
        ),
        "compressed_verify_seconds_mean": mean(
            row["compressed_verify_seconds"] for row in records
        ),
        "peak_rss_bytes_mean": mean(row["peak_rss_bytes"] for row in records),
        "proof_bytes": records[0]["compressed_proof_bytes_bincode"],
        "primary_constraints": records[0]["primary_constraints"],
        "raw_step_constraints": records[0]["raw_step_constraints"],
    }


def main():
    result = {
        "experiment": "CBD fused versus sequential versus restricted exact FP32",
        "backend": "Nova 0.73.0, Pallas/Vesta, Pedersen + IPA",
        "trajectory_metrics": trajectory_metrics(),
        "constraint_scaling": profile_constraints(),
        "nova_k2_n10": {
            "cbd_fused": timing_summary(
                "nova_cbd_local_decision_k2_n10_compressed*current.json"
            ),
            "cbd_sequential": timing_summary(
                "nova_cbd_sequential_k2_n10_rep*.json"
            ),
            "fp32_sequential": timing_summary(
                "nova_fp32_exact_n10_compressed*.json"
            ),
        },
        "causal_decision": (
            "CBD sequential remains below exact restricted FP32; "
            "the raw constraint advantage is not caused only by fusion."
        ),
        "caveats": [
            "CBD and FP32 still have different numeric semantics.",
            "FP32 remains restricted to positive normals and +0.",
            (
                "Fused and sequential interval widths are not pointwise ordered "
                "after rounding and clipping to [0,1]."
            ),
            "Timing measurements use three smoke repetitions.",
        ],
    }
    output = ROOT / "results" / "cbd_fusion_ablation.json"
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(output)


if __name__ == "__main__":
    main()

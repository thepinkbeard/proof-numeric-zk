#!/usr/bin/env python3
"""Consolida saídas do experimento sem dependências Python externas."""

import csv
import glob
import json
import math
import runpy
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESULTS = ROOT / "results"
DATA = ROOT / "data"
RESULTS.mkdir(exist_ok=True)

runpy.run_path(str(ROOT / "scripts" / "estimate_fp32_constraints.py"))["generate"]()


def load_json(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


constraint_rows = []
for path in sorted(RESULTS.glob("nova_k*_n10_constraints.json")):
    row = load_json(path)
    row["eta_precision_raw"] = row["p"] * row["k"] / row["raw_step_constraints"]
    row["eta_precision_augmented"] = (
        row["p"] * row["k"] / row["primary_constraints"]
    )
    constraint_rows.append(row)

with open(RESULTS / "constraint_table.csv", "w", newline="", encoding="utf-8") as handle:
    fields = [
        "p",
        "k",
        "raw_step_constraints",
        "modeled_range_checks",
        "modeled_multiplication_relations",
        "modeled_division_relations",
        "modeled_canonicality",
        "state_field_elements",
        "primary_constraints",
        "nova_structural_overhead_constraints",
        "peak_rss_bytes",
        "eta_precision_raw",
        "eta_precision_augmented",
    ]
    writer = csv.DictWriter(handle, fieldnames=fields, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(constraint_rows)

carry_rows = []
for path in sorted(RESULTS.glob("nova_k*_n10_carry.json")):
    row = load_json(path)
    row["eta_precision_raw"] = row["p"] * row["k"] / row["raw_step_constraints"]
    row["eta_precision_augmented"] = row["p"] * row["k"] / row["primary_constraints"]
    carry_rows.append(row)

with open(RESULTS / "carry_constraint_table.csv", "w", newline="", encoding="utf-8") as handle:
    fields = [
        "p",
        "k",
        "raw_step_constraints",
        "modeled_range_checks",
        "modeled_multiplication_relations",
        "modeled_division_relations",
        "modeled_canonicality",
        "state_field_elements",
        "primary_constraints",
        "nova_structural_overhead_constraints",
        "peak_rss_bytes",
        "eta_precision_raw",
        "eta_precision_augmented",
    ]
    writer = csv.DictWriter(handle, fieldnames=fields, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(carry_rows)

variant_paths = [
    RESULTS / "nova_k2_n10_constraints.json",
    RESULTS / "nova_k2_n10_nearest_even.json",
    RESULTS / "nova_k2_n10_carry.json",
    RESULTS / "nova_k2_n10_compensated.json",
    RESULTS / "nova_k2_n10_compensated_nearest_even.json",
]
variant_rows = []
for path in variant_paths:
    if path.exists():
        row = load_json(path)
        row["representation"] = row.get("representation", "standard")
        row["eta_precision_raw"] = row["p"] * row["k"] / row["raw_step_constraints"]
        row["eta_precision_augmented"] = row["p"] * row["k"] / row["primary_constraints"]
        variant_rows.append(row)

with open(RESULTS / "circuit_variants.csv", "w", newline="", encoding="utf-8") as handle:
    fields = [
        "representation",
        "rounding",
        "p",
        "k",
        "raw_step_constraints",
        "modeled_range_checks",
        "modeled_multiplication_relations",
        "modeled_division_relations",
        "modeled_canonicality",
        "state_field_elements",
        "primary_constraints",
        "compressed_proof_bytes_bincode",
        "peak_rss_bytes",
        "eta_precision_raw",
        "eta_precision_augmented",
    ]
    writer = csv.DictWriter(handle, fieldnames=fields, extrasaction="ignore")
    writer.writeheader()
    writer.writerows(variant_rows)

benchmark_groups = {}
for path in RESULTS.glob("nova_k2_n*_rep*.json"):
    row = load_json(path)
    benchmark_groups.setdefault(row["steps"], []).append(row)

benchmark_rows = []
for steps, rows in sorted(benchmark_groups.items()):
    def stats(field):
        values = [row[field] for row in rows]
        return statistics.mean(values), statistics.stdev(values) if len(values) > 1 else 0

    fold_mean, fold_sd = stats("prover_mean_step_seconds")
    total_mean, total_sd = stats("prover_total_seconds")
    verify_mean, verify_sd = stats("compressed_verify_seconds")
    compression_mean, compression_sd = stats("compression_seconds")
    rss_mean, rss_sd = stats("peak_rss_bytes")
    benchmark_rows.append(
        {
            "steps": steps,
            "repetitions": len(rows),
            "fold_mean_seconds": fold_mean,
            "fold_sd_seconds": fold_sd,
            "total_mean_seconds": total_mean,
            "total_sd_seconds": total_sd,
            "compression_mean_seconds": compression_mean,
            "compression_sd_seconds": compression_sd,
            "compressed_verify_mean_seconds": verify_mean,
            "compressed_verify_sd_seconds": verify_sd,
            "proof_bytes": rows[0]["compressed_proof_bytes_bincode"],
            "peak_rss_mean_bytes": rss_mean,
            "peak_rss_sd_bytes": rss_sd,
        }
    )

with open(RESULTS / "nova_benchmark_summary.csv", "w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=benchmark_rows[0].keys())
    writer.writeheader()
    writer.writerows(benchmark_rows)

selected = {
    "f32",
    "f64",
    "Q16.16_truncate",
    "Q16.16_nearest_even",
    "Q32.32_truncate",
    "Q32.32_nearest_even",
    "limbs_16x2_truncate",
    "limbs_16x2_nearest_even",
    "limbs_16x3_nearest_even",
    "limbs_16x4_nearest_even",
    "limbs_16x1_compensated",
    "limbs_16x2_compensated",
}
numeric_rows = []
for path in sorted(DATA.glob("trajectory_r*_n10000.summary.json")):
    for row in load_json(path):
        if row["format"] in selected:
            numeric_rows.append(row)

with open(RESULTS / "numeric_summary_n10000.csv", "w", newline="", encoding="utf-8") as handle:
    fields = list(numeric_rows[0].keys())
    writer = csv.DictWriter(handle, fieldnames=fields)
    writer.writeheader()
    writer.writerows(numeric_rows)


def svg_bars(path, title, labels, values, y_label):
    width, height = 980, 520
    left, right, top, bottom = 80, 30, 55, 150
    plot_w, plot_h = width - left - right, height - top - bottom
    maximum = max(values) or 1
    bar_w = plot_w / max(len(values), 1) * 0.68
    elements = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">',
        '<rect width="100%" height="100%" fill="white"/>',
        f'<text x="{width/2}" y="28" text-anchor="middle" font-family="sans-serif" font-size="20">{title}</text>',
        f'<text x="18" y="{top + plot_h/2}" transform="rotate(-90 18 {top + plot_h/2})" text-anchor="middle" font-family="sans-serif" font-size="13">{y_label}</text>',
        f'<line x1="{left}" y1="{top}" x2="{left}" y2="{top+plot_h}" stroke="#333"/>',
        f'<line x1="{left}" y1="{top+plot_h}" x2="{left+plot_w}" y2="{top+plot_h}" stroke="#333"/>',
    ]
    for index, (label, value) in enumerate(zip(labels, values)):
        x = left + (index + 0.5) * plot_w / len(values) - bar_w / 2
        h = value / maximum * plot_h
        y = top + plot_h - h
        elements.append(
            f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_w:.1f}" height="{h:.1f}" fill="#3569a8"/>'
        )
        elements.append(
            f'<text x="{x+bar_w/2:.1f}" y="{y-6:.1f}" text-anchor="middle" font-family="sans-serif" font-size="11">{value:.4g}</text>'
        )
        elements.append(
            f'<text x="{x+bar_w/2:.1f}" y="{top+plot_h+12}" transform="rotate(45 {x+bar_w/2:.1f} {top+plot_h+12})" text-anchor="start" font-family="sans-serif" font-size="11">{label}</text>'
        )
    elements.append("</svg>")
    path.write_text("\n".join(elements), encoding="utf-8")


svg_bars(
    RESULTS / "constraints_by_k.svg",
    "Constraints da step circuit por número de limbs",
    [f"16×{row['k']}" for row in constraint_rows],
    [row["raw_step_constraints"] for row in constraint_rows],
    "constraints R1CS",
)

r37 = [
    row
    for row in numeric_rows
    if row["r"] == "3.7"
    and row["format"]
    in {
        "f32",
        "f64",
        "Q16.16_truncate",
        "Q32.32_truncate",
        "limbs_16x3_nearest_even",
        "limbs_16x4_nearest_even",
        "limbs_16x2_compensated",
    }
]
svg_bars(
    RESULTS / "first_divergence_r3_7.svg",
    "Primeiro erro absoluto > 10⁻⁶ (r=3.7)",
    [row["format"] for row in r37],
    [
        row["first_error_above_1e_6"]
        if row["first_error_above_1e_6"] is not None
        else 10000
        for row in r37
    ],
    "iteração",
)

print(f"{len(numeric_rows)} resumos numéricos consolidados")
print(f"{len(benchmark_rows)} horizontes Nova consolidados")

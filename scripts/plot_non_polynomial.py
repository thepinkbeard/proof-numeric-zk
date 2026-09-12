#!/usr/bin/env python3
"""Generate dependency-free SVG figures for function-eval results.

The script reads only raw.csv and summary.json. It does not embed experimental
values in the plotting code and refuses to overwrite an existing output
directory, so every figure remains traceable to one immutable run directory.
"""

from __future__ import annotations

import argparse
import csv
import html
import json
import math
from collections import defaultdict
from fractions import Fraction
from pathlib import Path

WIDTH = 960
HEIGHT = 600
LEFT = 92
RIGHT = 28
TOP = 48
BOTTOM = 78
COLORS = {
    "taylor": "#2563eb",
    "pade-diagonal": "#dc2626",
    "gauss-legendre-implicit": "#059669",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser.parse_args()


def esc(value: object) -> str:
    return html.escape(str(value))


def finite_positive(value: float) -> float:
    return max(value, 1e-300)


def transform(value: float, log_scale: bool) -> float:
    return math.log10(finite_positive(value)) if log_scale else value


def ticks(low: float, high: float, count: int = 6) -> list[float]:
    if math.isclose(low, high):
        return [low]
    return [low + index * (high - low) / (count - 1) for index in range(count)]


def plot_svg(
    path: Path,
    title: str,
    x_label: str,
    y_label: str,
    series: dict[str, list[tuple[float, float]]],
    *,
    log_x: bool = False,
    log_y: bool = False,
    connect: bool = True,
) -> None:
    clean = {
        name: [(x, y) for x, y in values if math.isfinite(x) and math.isfinite(y)]
        for name, values in series.items()
    }
    clean = {name: values for name, values in clean.items() if values}
    if not clean:
        raise ValueError(f"no data for {title}")
    all_x = [transform(x, log_x) for values in clean.values() for x, _ in values]
    all_y = [transform(y, log_y) for values in clean.values() for _, y in values]
    x_min, x_max = min(all_x), max(all_x)
    y_min, y_max = min(all_y), max(all_y)
    if math.isclose(x_min, x_max):
        x_min -= 0.5
        x_max += 0.5
    if math.isclose(y_min, y_max):
        y_min -= 0.5
        y_max += 0.5
    x_pad = (x_max - x_min) * 0.04
    y_pad = (y_max - y_min) * 0.08
    x_min -= x_pad
    x_max += x_pad
    y_min -= y_pad
    y_max += y_pad

    def sx(value: float) -> float:
        normalized = (transform(value, log_x) - x_min) / (x_max - x_min)
        return LEFT + normalized * (WIDTH - LEFT - RIGHT)

    def sy(value: float) -> float:
        normalized = (transform(value, log_y) - y_min) / (y_max - y_min)
        return HEIGHT - BOTTOM - normalized * (HEIGHT - TOP - BOTTOM)

    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">',
        f'<rect x="0" y="0" width="{WIDTH}" height="{HEIGHT}" fill="#ffffff"/>',
        '<style>text{font-family:Inter,Arial,sans-serif;fill:#172033}.axis{stroke:#526071;stroke-width:1}.grid{stroke:#d9e0e8;stroke-width:1}.label{font-size:13px}.title{font-size:20px;font-weight:600}.legend{font-size:12px}</style>',
        f'<text x="{WIDTH/2}" y="28" text-anchor="middle" class="title">{esc(title)}</text>',
    ]
    for value in ticks(x_min, x_max):
        x = LEFT + (value - x_min) / (x_max - x_min) * (WIDTH - LEFT - RIGHT)
        label = f"1e{value:.0f}" if log_x else f"{value:.3g}"
        parts.append(f'<line x1="{x:.2f}" y1="{TOP}" x2="{x:.2f}" y2="{HEIGHT-BOTTOM}" class="grid"/>')
        parts.append(f'<text x="{x:.2f}" y="{HEIGHT-BOTTOM+22}" text-anchor="middle" class="label">{label}</text>')
    for value in ticks(y_min, y_max):
        y = HEIGHT - BOTTOM - (value - y_min) / (y_max - y_min) * (HEIGHT - TOP - BOTTOM)
        label = f"1e{value:.0f}" if log_y else f"{value:.3g}"
        parts.append(f'<line x1="{LEFT}" y1="{y:.2f}" x2="{WIDTH-RIGHT}" y2="{y:.2f}" class="grid"/>')
        parts.append(f'<text x="{LEFT-12}" y="{y+4:.2f}" text-anchor="end" class="label">{label}</text>')
    parts.extend([
        f'<line x1="{LEFT}" y1="{HEIGHT-BOTTOM}" x2="{WIDTH-RIGHT}" y2="{HEIGHT-BOTTOM}" class="axis"/>',
        f'<line x1="{LEFT}" y1="{TOP}" x2="{LEFT}" y2="{HEIGHT-BOTTOM}" class="axis"/>',
        f'<text x="{(LEFT+WIDTH-RIGHT)/2}" y="{HEIGHT-24}" text-anchor="middle" class="label">{esc(x_label)}</text>',
        f'<text x="22" y="{(TOP+HEIGHT-BOTTOM)/2}" transform="rotate(-90 22 {(TOP+HEIGHT-BOTTOM)/2})" text-anchor="middle" class="label">{esc(y_label)}</text>',
    ])
    methods = sorted({name.split(" | ")[0] for name in clean})
    legend_x = LEFT + 12
    for index, method in enumerate(methods):
        color = COLORS.get(method, "#7c3aed")
        x = legend_x + index * 245
        parts.append(f'<circle cx="{x}" cy="42" r="3.4" fill="{color}"/>')
        parts.append(f'<text x="{x+9}" y="46" class="legend">{esc(method)}</text>')
    for name, values in sorted(clean.items()):
        method = name.split(" | ")[0]
        color = COLORS.get(method, "#7c3aed")
        ordered = sorted(values)
        if connect and len(ordered) > 1:
            points = " ".join(f"{sx(x):.2f},{sy(y):.2f}" for x, y in ordered)
            parts.append(f'<polyline points="{points}" fill="none" stroke="{color}" stroke-width="1.6" opacity="0.72"/>')
        for x, y in ordered:
            parts.append(f'<circle cx="{sx(x):.2f}" cy="{sy(y):.2f}" r="3.2" fill="{color}"><title>{esc(name)}: {x:.6g}, {y:.6g}</title></circle>')
    parts.append("</svg>")
    path.write_text("\n".join(parts), encoding="utf-8")


def method_series(summary: list[dict], x_field: str, y_field: str) -> dict[str, list[tuple[float, float]]]:
    series: dict[str, list[tuple[float, float]]] = defaultdict(list)
    for row in summary:
        key = f"{row['approximation_method']} | {row['function']} | {row['rounding']}"
        series[key].append((float(row[x_field]), float(row[y_field])))
    return dict(series)


def flip_rate(row: dict) -> float:
    opportunities = int(row["decision_opportunities"]) - int(row.get("reference_inconclusive_count", 0))
    return int(row["predicate_flip_count"]) / opportunities if opportunities else float("nan")


def stability_by_margin(raw_rows: list[dict]) -> dict[str, list[tuple[float, float]]]:
    buckets: dict[tuple[str, int], list[int]] = defaultdict(list)
    for row in raw_rows:
        margin = abs(float(Fraction(row["decision_margin"])))
        if margin <= 0:
            bucket = -20
        else:
            bucket = max(-20, min(4, math.floor(math.log2(margin))))
        stable = int(row["decision_certification"] == "certified-stable")
        buckets[(row["approximation_method"], bucket)].append(stable)
    series: dict[str, list[tuple[float, float]]] = defaultdict(list)
    for (method, bucket), values in buckets.items():
        series[method].append((2.0**bucket, sum(values) / len(values)))
    return dict(series)


def pareto_series(summary: list[dict]) -> dict[str, list[tuple[float, float]]]:
    points = [
        (int(row["operation_count_per_evaluation"]), float(row["max_absolute_error"]), row)
        for row in summary
    ]
    frontier = []
    for cost, error, row in points:
        dominated = any(
            other_cost <= cost
            and other_error <= error
            and (other_cost < cost or other_error < error)
            for other_cost, other_error, _ in points
        )
        if not dominated:
            frontier.append((cost, error, row))
    series: dict[str, list[tuple[float, float]]] = defaultdict(list)
    for cost, error, row in frontier:
        key = f"{row['approximation_method']} | Pareto"
        series[key].append((cost, error))
    return dict(series)


def main() -> None:
    args = parse_args()
    if args.output_dir.exists():
        raise SystemExit(f"output directory already exists: {args.output_dir}")
    summary = json.loads((args.input_dir / "summary.json").read_text(encoding="utf-8"))
    with (args.input_dir / "raw.csv").open(newline="", encoding="utf-8") as handle:
        raw_rows = list(csv.DictReader(handle))
    args.output_dir.mkdir(parents=True)

    plot_svg(args.output_dir / "01_error_vs_precision.svg", "Maximum error vs precision", "fractional bits", "maximum absolute error", method_series(summary, "fractional_bits", "max_absolute_error"), log_y=True)
    plot_svg(args.output_dir / "02_error_vs_order.svg", "Maximum error vs approximation order", "order", "maximum absolute error", method_series(summary, "approximation_order", "max_absolute_error"), log_y=True)
    accuracy_cost = defaultdict(list)
    depth_accuracy = defaultdict(list)
    flips = defaultdict(list)
    for row in summary:
        method = row["approximation_method"]
        accuracy = -math.log2(finite_positive(float(row["max_absolute_error"])))
        key = f"{method} | {row['function']}"
        accuracy_cost[key].append((int(row["operation_count_per_evaluation"]), accuracy))
        depth_accuracy[key].append((int(row["numerical_multiplicative_depth"]), accuracy))
        flips[key].append((int(row["fractional_bits"]), flip_rate(row)))
    plot_svg(args.output_dir / "03_cost_vs_accuracy.svg", "Operation cost vs accuracy", "operation count", "absolute accuracy bits", dict(accuracy_cost))
    plot_svg(args.output_dir / "04_depth_vs_accuracy.svg", "Numerical multiplicative depth vs accuracy", "numerical multiplicative depth", "absolute accuracy bits", dict(depth_accuracy))
    plot_svg(args.output_dir / "05_flip_rate_vs_precision.svg", "Predicate flip rate vs precision", "fractional bits", "predicate flip rate", dict(flips))
    plot_svg(args.output_dir / "06_stability_vs_margin.svg", "Certified decision stability vs margin", "decision margin", "certified fraction", stability_by_margin(raw_rows), log_x=True)
    plot_svg(args.output_dir / "07_pareto_cost_vs_error.svg", "Pareto frontier: operation cost vs error", "operation count", "maximum absolute error", pareto_series(summary), log_y=True, connect=False)


if __name__ == "__main__":
    main()

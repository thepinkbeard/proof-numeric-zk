#!/usr/bin/env python3
"""Intervalos de confiança e ordenação de tempos, a partir de `results/timing/`.

O que 0006 fez foi tirar a média de 3 repetições por representação e comparar as
médias. Com 3 pontos e variância de máquina, a diferença observada não separa
nada, e o `report.md` já dizia isso. Aqui há duas correções.

A primeira é o número de repetições, que passa a ser suficiente para bootstrap.

A segunda importa mais e é de desenho. Como `run_timing_benchmarks.sh` intercala
as representações — cada rodada roda todas —, a comparação pode ser **pareada**
dentro da rodada. Se a máquina esquentou na rodada 17, todas as representações
daquela rodada ficaram lentas juntas, e a diferença entre elas dentro da rodada
não sofre. Comparar diferenças pareadas remove a variância entre rodadas, que é
justamente a que dominava e impedia ordenar.

Bootstrap e não teste t: os tempos são assimétricos à direita, com cauda de
interrupções do sistema, e a hipótese de normalidade do t não se sustenta. O
bootstrap percentil não precisa dela.
"""

from __future__ import annotations

import json
import random
from collections import defaultdict
from pathlib import Path
from statistics import mean, median, stdev

ROOT = Path(__file__).resolve().parents[1]
TIMING = ROOT / "results" / "timing"
OUTPUT = ROOT / "results" / "timing_summary.json"

BOOTSTRAP_SAMPLES = 20_000
SEED = 20260730

METRICS = [
    ("setup_seconds", "setup"),
    ("prover_total_seconds", "prova"),
    ("prover_mean_step_seconds", "prova por passo"),
    ("recursive_verify_seconds", "verificação recursiva"),
    ("compression_seconds", "compressão"),
    ("compressed_verify_seconds", "verificação comprimida"),
]


def load() -> dict[str, dict[int, dict]]:
    runs: dict[str, dict[int, dict]] = defaultdict(dict)
    for path in sorted(TIMING.glob("*_round*.json")):
        representation, _, round_part = path.stem.rpartition("_round")
        runs[representation][int(round_part)] = json.loads(path.read_text())
    return runs


def bootstrap_ci(
    values: list[float], statistic, confidence: float = 0.95
) -> tuple[float, float]:
    """Intervalo percentil por reamostragem com reposição."""
    if len(values) < 2:
        return (float("nan"), float("nan"))
    rng = random.Random(SEED)
    size = len(values)
    estimates = []
    for _ in range(BOOTSTRAP_SAMPLES):
        sample = [values[rng.randrange(size)] for _ in range(size)]
        estimates.append(statistic(sample))
    estimates.sort()
    tail = (1.0 - confidence) / 2.0
    low = estimates[int(tail * BOOTSTRAP_SAMPLES)]
    high = estimates[min(int((1.0 - tail) * BOOTSTRAP_SAMPLES), BOOTSTRAP_SAMPLES - 1)]
    return (low, high)


def describe(values: list[float]) -> dict:
    low, high = bootstrap_ci(values, mean)
    return {
        "n": len(values),
        "mean": mean(values),
        "median": median(values),
        "stdev": stdev(values) if len(values) > 1 else 0.0,
        "min": min(values),
        "max": max(values),
        "ci95_low": low,
        "ci95_high": high,
        # Coeficiente de variação: quanto do tempo é ruído da máquina. Acima de
        # uns 10% qualquer ordenação fica frágil, e é honesto dizer isso.
        "cv": (stdev(values) / mean(values)) if len(values) > 1 and mean(values) else 0.0,
    }


def paired_difference(
    runs: dict[str, dict[int, dict]], left: str, right: str, field: str
) -> dict | None:
    """Diferença `left - right` medida dentro de cada rodada.

    Só as rodadas em que ambas as representações produziram o campo entram, para
    que o pareamento seja real.
    """
    rounds = sorted(set(runs[left]) & set(runs[right]))
    differences = []
    for index in rounds:
        a = runs[left][index].get(field)
        b = runs[right][index].get(field)
        if a is None or b is None:
            continue
        differences.append(a - b)
    if len(differences) < 2:
        return None

    low, high = bootstrap_ci(differences, mean)
    # A ordenação só é afirmável se o intervalo inteiro ficar de um lado do zero.
    # Se ele cruza zero, o dado não distingue as duas, e o resultado honesto é
    # "não distingue" — não "são iguais" nem "a média sugere que".
    if low > 0:
        verdict = f"{left} mais lento que {right}"
    elif high < 0:
        verdict = f"{left} mais rápido que {right}"
    else:
        verdict = "indistinguível"
    return {
        "left": left,
        "right": right,
        "field": field,
        "pairs": len(differences),
        "mean_difference": mean(differences),
        "median_difference": median(differences),
        "ci95_low": low,
        "ci95_high": high,
        "separated": low > 0 or high < 0,
        "verdict": verdict,
    }


def main() -> None:
    if not TIMING.exists():
        raise SystemExit(
            f"{TIMING} não existe; rode scripts/run_timing_benchmarks.sh antes"
        )
    runs = load()
    if not runs:
        raise SystemExit(f"nenhum resultado em {TIMING}")

    representations = sorted(runs)
    summary: dict[str, dict] = {}
    for representation in representations:
        entries = runs[representation]
        summary[representation] = {"rounds": len(entries), "metrics": {}}
        for field, label in METRICS:
            values = [
                entry[field]
                for entry in entries.values()
                if entry.get(field) is not None
            ]
            if values:
                summary[representation]["metrics"][field] = describe(values) | {
                    "label": label
                }

    comparisons = []
    for field, _ in METRICS:
        for index, left in enumerate(representations):
            for right in representations[index + 1 :]:
                result = paired_difference(runs, left, right, field)
                if result is not None:
                    comparisons.append(result)

    OUTPUT.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "bootstrap_samples": BOOTSTRAP_SAMPLES,
                "seed": SEED,
                "method": "bootstrap percentil, comparação pareada por rodada",
                "representations": summary,
                "paired_comparisons": comparisons,
            },
            indent=2,
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )

    for representation in representations:
        entry = summary[representation]
        print(f"\n=== {representation} ({entry['rounds']} rodadas) ===")
        for field, label in METRICS:
            stats = entry["metrics"].get(field)
            if not stats:
                continue
            print(
                f"  {label:24s} {stats['mean']:8.4f}s  "
                f"IC95 [{stats['ci95_low']:.4f}, {stats['ci95_high']:.4f}]  "
                f"cv {100 * stats['cv']:.1f}%"
            )

    print("\n=== comparações pareadas ===")
    separated = [item for item in comparisons if item["separated"]]
    print(f"{len(separated)} de {len(comparisons)} pares separam com IC95\n")
    for field, label in METRICS:
        items = [item for item in comparisons if item["field"] == field]
        if not items:
            continue
        print(f"  -- {label} --")
        for item in sorted(items, key=lambda entry: entry["mean_difference"]):
            mark = "sim" if item["separated"] else "NAO"
            print(
                f"    [{mark}] {item['left']:15s} - {item['right']:15s} "
                f"{item['mean_difference']:+9.4f}s  "
                f"IC95 [{item['ci95_low']:+.4f}, {item['ci95_high']:+.4f}]"
            )

    print(f"\n{OUTPUT}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Junta custo de circuito e cobertura de decisões numa fronteira de Pareto.

O custo vem de results/nova_sweep/*.json, um arquivo por configuração, e a
cobertura de results/frontier_coverage.json. A chave é o profile_hash_hex, que
os dois lados derivam do mesmo ExperimentProfile em numeric-core.

Junção por hash e não por rótulo: o rótulo de representação é o mesmo em
workloads diferentes, de propósito, e juntar por ele misturaria experimentos.
O preço é que uma divergência de um único campo do perfil produz dois hashes e
some da fronteira em silêncio — por isso as duas listas de órfãos são relatadas
e o script falha quando a junção fica abaixo do limite esperado.

Saída:
  results/pareto_frontier.json  pontos, fronteiras e dominância por regime
  results/pareto_frontier.csv   a mesma tabela, para inspeção manual
"""

from __future__ import annotations

import csv
import json
import sys
from collections import defaultdict
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SWEEP_DIR = ROOT / "results" / "nova_sweep"
COVERAGE = ROOT / "results" / "frontier_coverage.json"
OUTPUT_JSON = ROOT / "results" / "pareto_frontier.json"
OUTPUT_CSV = ROOT / "results" / "pareto_frontier.csv"

# Abaixo disso a junção está quebrada, não incompleta: vale interromper em vez
# de publicar uma fronteira construída sobre um punhado de pontos.
MINIMUM_JOIN_RATE = 0.80


def load_json(path: Path):
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def load_cost() -> tuple[dict[str, dict], list[str]]:
    """Custo por hash de perfil.

    Configurações repetidas precisam concordar em raw_step_constraints: o custo
    de uma etapa é determinístico, e discordância indica que dois perfis
    diferentes colidiram no mesmo hash — erro grave, não ruído de medição.
    """
    costs: dict[str, dict] = {}
    conflicts: list[str] = []
    for path in sorted(SWEEP_DIR.glob("*.json")):
        record = load_json(path)
        digest = record.get("profile_hash_hex")
        if not digest:
            continue
        raw = record["raw_step_constraints"]
        if digest in costs and costs[digest]["raw_step_constraints"] != raw:
            conflicts.append(
                f"{digest[:12]}: {costs[digest]['source']}={costs[digest]['raw_step_constraints']} "
                f"vs {path.name}={raw}"
            )
            continue
        costs[digest] = {
            "raw_step_constraints": raw,
            "total_constraints": record.get("total_constraints"),
            "state_field_elements": record.get("state_field_elements"),
            "prove_seconds": record.get("prove_seconds"),
            "source": path.name,
        }
    return costs, conflicts


def load_link_cost(path: Path) -> int | None:
    """Custo medido do vínculo de escalonamento, em constraints.

    Vem de `escalation-cost`, que sintetiza o circuito de vínculo isolado. Se o
    arquivo não existir, o termo volta a ser omitido em vez de estimado.
    """
    if not path.exists():
        return None
    points = json.loads(path.read_text())["points"]
    # O vínculo é praticamente plano na cadeia 16→32→48→64; o máximo é o valor
    # conservador a usar, e a diferença entre os pontos é de uma constraint.
    return max(point["link_constraints"] for point in points)


def escalation_cost(
    costs: dict[str, dict],
    coverage: list[dict],
    point: dict,
    link_cost: int | None,
) -> float | None:
    """Custo esperado com escalonamento: `C_p + (A_p/N) * (C_link + C_p')`.

    `p'` é o próximo perfil da mesma representação e workload, uma precisão
    acima. Sem perfil maior disponível, o termo não é estimado: a abstenção
    residual não teria para onde escalar e um valor inventado aqui viraria
    conclusão adiante.

    `C_link` é **medido**, não modelado: é o circuito de `escalation.rs` que
    amarra a reprova ao mesmo compromisso da entrada privada. Sem ele o
    escalonamento não seria uma recuperação, seria uma oportunidade de trocar a
    entrada depois de ver a abstenção.
    """
    if not point["opportunities"] or link_cost is None:
        return None
    candidates = [
        other
        for other in coverage
        if other["representation"] == point["representation"]
        and other["workload"] == point["workload"]
        and other["regime"] == point["regime"]
        and other["threshold"] == point["threshold"]
        and other["rounding"] == point["rounding"]
        and other["exponent_lanes"] == point["exponent_lanes"]
        and other["precision_bits"] > point["precision_bits"]
        and other["profile_hash_hex"] in costs
    ]
    if not candidates:
        return None
    successor = min(candidates, key=lambda item: item["precision_bits"])
    rate = point["abstained"] / point["opportunities"]
    successor_cost = costs[successor["profile_hash_hex"]]["raw_step_constraints"]
    return point["cost"] + rate * (link_cost + successor_cost)


def pareto_front(points: list[dict]) -> list[dict]:
    """Pontos não dominados em (custo por decisão correta, taxa de erro).

    Dois eixos e não um: ordenar só por custo por decisão correta faria um
    formato que erra metade das decisões parecer barato, porque o denominador já
    desconta os erros mas o consumidor não sabe quais decisões descontar. A taxa
    de decisões incorretas entra como eixo próprio para que a dominância exija
    ser melhor nos dois.
    """
    usable = [point for point in points if point["cost_per_correct"] is not None]
    front = []
    for candidate in usable:
        dominated = any(
            other is not candidate
            and other["cost_per_correct"] <= candidate["cost_per_correct"]
            and other["unsound_rate"] <= candidate["unsound_rate"]
            and (
                other["cost_per_correct"] < candidate["cost_per_correct"]
                or other["unsound_rate"] < candidate["unsound_rate"]
            )
            for other in usable
        )
        if not dominated:
            front.append(candidate)
    return sorted(front, key=lambda point: point["cost_per_correct"])


def fronts_by_guarantee(points: list[dict]) -> dict[str, list[dict]]:
    """Uma fronteira por classe de garantia, nunca uma fronteira única.

    Pondo as duas classes na mesma relação de dominância, um ponto não
    certificado que por acaso acertou tudo nesta trajetória tem taxa de erro
    zero e custo menor, e portanto domina **todos** os pontos certificados — que
    somem da fronteira. Foi o que aconteceu na primeira versão deste script: o
    afim diádico devolvia `standard p16 truncate` como único ponto e nenhuma
    alternativa certificada, sugerindo que certificar nunca compensa.

    A conclusão era um artefato da relação de ordem. As duas classes não vendem
    o mesmo produto: uma garante para toda entrada, a outra foi observada numa
    trajetória. Comparar dentro de cada classe e reportar a razão entre elas
    separa o que é medida do que é escolha de quem lê.
    """
    return {
        "certificada": pareto_front([point for point in points if point["certified"]]),
        "observada": pareto_front([point for point in points if not point["certified"]]),
    }


def escape(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def svg_scatter(
    path: Path,
    title: str,
    panels: list[tuple[str, list[dict]]],
    x_label: str,
    y_label: str,
) -> None:
    """Dispersão log-x com um painel por regime.

    O eixo de custo é logarítmico porque a fronteira cobre de 0,11 a mais de 100
    constraints por decisão correta; em escala linear os regimes fáceis viram
    uma única coluna colada no zero.
    """
    panel_width, panel_height = 300, 210
    left, top, right, bottom = 54, 34, 16, 46
    columns = min(3, len(panels))
    rows = (len(panels) + columns - 1) // columns
    width = columns * (panel_width + left + right)
    height = rows * (panel_height + top + bottom) + 34

    values = [point["cost_per_correct"] for _, points in panels for point in points]
    if not values:
        return
    import math

    low = math.log10(max(min(values), 1e-3))
    high = math.log10(max(values))
    if high - low < 0.5:
        low, high = low - 0.25, high + 0.25

    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" fill="white"/>',
        f'<text x="{width/2}" y="21" text-anchor="middle" font-family="sans-serif" '
        f'font-size="15" font-weight="bold">{escape(title)}</text>',
    ]
    for index, (label, points) in enumerate(panels):
        column, row = index % columns, index // columns
        origin_x = column * (panel_width + left + right) + left
        origin_y = row * (panel_height + top + bottom) + top + 34
        parts.append(
            f'<text x="{origin_x + panel_width/2}" y="{origin_y - 10}" text-anchor="middle" '
            f'font-family="sans-serif" font-size="12">{escape(label)}</text>'
        )
        parts.append(
            f'<line x1="{origin_x}" y1="{origin_y}" x2="{origin_x}" '
            f'y2="{origin_y + panel_height}" stroke="#333"/>'
        )
        parts.append(
            f'<line x1="{origin_x}" y1="{origin_y + panel_height}" '
            f'x2="{origin_x + panel_width}" y2="{origin_y + panel_height}" stroke="#333"/>'
        )
        for fraction in (0.0, 0.25, 0.5, 0.75, 1.0):
            y = origin_y + panel_height - fraction * panel_height
            parts.append(
                f'<line x1="{origin_x}" y1="{y:.1f}" x2="{origin_x + panel_width}" '
                f'y2="{y:.1f}" stroke="#eee"/>'
            )
            parts.append(
                f'<text x="{origin_x - 6}" y="{y + 4:.1f}" text-anchor="end" '
                f'font-family="sans-serif" font-size="9">{fraction*100:.0f}%</text>'
            )
        for decade in range(int(low), int(high) + 2):
            if not low <= decade <= high:
                continue
            x = origin_x + (decade - low) / (high - low) * panel_width
            parts.append(
                f'<line x1="{x:.1f}" y1="{origin_y}" x2="{x:.1f}" '
                f'y2="{origin_y + panel_height}" stroke="#eee"/>'
            )
            parts.append(
                f'<text x="{x:.1f}" y="{origin_y + panel_height + 15}" text-anchor="middle" '
                f'font-family="sans-serif" font-size="9">1e{decade}</text>'
            )
        for point in points:
            x = origin_x + (math.log10(max(point["cost_per_correct"], 1e-3)) - low) / (
                high - low
            ) * panel_width
            y = origin_y + panel_height - point["unsound_rate"] * panel_height
            if point["certified"]:
                parts.append(
                    f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4" fill="#1a6ebd" '
                    f'fill-opacity="0.75"/>'
                )
            else:
                parts.append(
                    f'<rect x="{x-3.5:.1f}" y="{y-3.5:.1f}" width="7" height="7" '
                    f'fill="#c0392b" fill-opacity="0.7"/>'
                )
        parts.append(
            f'<text x="{origin_x + panel_width/2}" y="{origin_y + panel_height + 32}" '
            f'text-anchor="middle" font-family="sans-serif" font-size="10">'
            f'{escape(x_label)}</text>'
        )
        parts.append(
            f'<text x="{origin_x - 40}" y="{origin_y + panel_height/2}" '
            f'transform="rotate(-90 {origin_x - 40} {origin_y + panel_height/2})" '
            f'text-anchor="middle" font-family="sans-serif" font-size="10">'
            f'{escape(y_label)}</text>'
        )
    parts.append(
        f'<circle cx="{width/2 - 130}" cy="{height - 8}" r="4" fill="#1a6ebd"/>'
        f'<text x="{width/2 - 120}" y="{height - 4}" font-family="sans-serif" font-size="10">'
        f'certificada (erro nulo por construcao)</text>'
        f'<rect x="{width/2 + 78}" y="{height - 12}" width="7" height="7" fill="#c0392b"/>'
        f'<text x="{width/2 + 90}" y="{height - 4}" font-family="sans-serif" font-size="10">'
        f'nao certificada</text>'
    )
    parts.append("</svg>")
    path.write_text("\n".join(parts), encoding="utf-8")


def main() -> int:
    if not COVERAGE.exists():
        print(f"cobertura ausente: {COVERAGE}", file=sys.stderr)
        print("rode: cargo run --release -p experiment-runner --bin frontier-eval", file=sys.stderr)
        return 1
    if not SWEEP_DIR.exists():
        print(f"varredura de custo ausente: {SWEEP_DIR}", file=sys.stderr)
        print("rode: scripts/run_pareto_sweep.sh", file=sys.stderr)
        return 1

    costs, conflicts = load_cost()
    coverage = load_json(COVERAGE)["records"]

    points = []
    coverage_orphans = []
    for record in coverage:
        digest = record["profile_hash_hex"]
        cost = costs.get(digest)
        if cost is None:
            coverage_orphans.append(record)
            continue
        raw = cost["raw_step_constraints"]
        correct = record["correct_definite"]
        points.append(
            {
                "profile_hash_hex": digest,
                "workload": record["workload"],
                "regime": record["regime"],
                "representation": record["representation"],
                "family": record["family"],
                "bound_mode": record["bound_mode"],
                "topology": record["topology"],
                "certified": record["certified"],
                "rounding": record["rounding"],
                "precision_bits": record["precision_bits"],
                "exponent_lanes": record["exponent_lanes"],
                "threshold": record["threshold"],
                "parameters": record["parameters"],
                "cost": raw,
                "total_constraints": cost["total_constraints"],
                "opportunities": record["opportunities"],
                "definite": record["definite"],
                "abstained": record["abstained"],
                "unsound_definite": record["unsound_definite"],
                "correct_definite": correct,
                "availability": record["availability"],
                "unsound_rate": record["unsound_rate"] or 0.0,
                "definitive_horizon": record["definitive_horizon"],
                "cost_per_correct": raw / correct if correct else None,
            }
        )

    joined_hashes = {point["profile_hash_hex"] for point in points}
    cost_orphans = sorted(set(costs) - joined_hashes)

    link_cost = load_link_cost(ROOT / "results" / "escalation_link_cost.json")
    for point in points:
        point["expected_cost_with_escalation"] = escalation_cost(
            costs, coverage, point, link_cost
        )

    join_rate = len(points) / len(coverage) if coverage else 0.0

    fronts = {}
    dominance = []
    groups = defaultdict(list)
    for point in points:
        groups[(point["workload"], point["regime"], point["threshold"])].append(point)
    for key, group in sorted(groups.items()):
        by_class = fronts_by_guarantee(group)
        front = by_class["certificada"] + by_class["observada"]
        label = "|".join(key)
        fronts[label] = {
            name: [point["profile_hash_hex"] for point in members]
            for name, members in by_class.items()
        }
        if not front:
            continue
        # Três vencedores, não um, porque "não errou" significa duas coisas
        # diferentes conforme a família.
        #
        # Num formato certificado, taxa de erro nula é propriedade do enclosure:
        # vale para qualquer entrada, e é o que o circuito prova. Num formato
        # não certificado é observação sobre esta trajetória: no afim diádico o
        # ponto fixo com truncagem acerta tudo porque nunca arredonda para cima,
        # e o mesmo formato com par mais próximo erra 983 de 1.001. Colapsar os
        # dois casos numa coluna só transformaria sorte em garantia, que é
        # exatamente o erro que o capítulo existe para desfazer.
        certified = [
            point for point in by_class["certificada"] if point["correct_definite"]
        ]
        observed = [
            point
            for point in by_class["observada"]
            if point["unsound_rate"] == 0.0 and point["correct_definite"]
        ]

        def summarize(point: dict | None) -> dict | None:
            if point is None:
                return None
            return {
                "representation": point["representation"],
                "precision_bits": point["precision_bits"],
                "rounding": point["rounding"],
                "cost": point["cost"],
                "cost_per_correct": point["cost_per_correct"],
                "unsound_rate": point["unsound_rate"],
                "availability": point["availability"],
                "definitive_horizon": point["definitive_horizon"],
                "guarantee": "certificada" if point["certified"] else "observada",
            }

        by_cost = lambda point: point["cost_per_correct"]  # noqa: E731
        cheapest_certified = min(certified, key=by_cost) if certified else None
        cheapest_observed = min(observed, key=by_cost) if observed else None
        dominance.append(
            {
                "workload": key[0],
                "regime": key[1],
                "threshold": key[2],
                "front_size_certified": len(by_class["certificada"]),
                "front_size_observed": len(by_class["observada"]),
                "cheapest_overall": summarize(min(front, key=by_cost)),
                "cheapest_certified": summarize(cheapest_certified),
                "cheapest_observed_sound": summarize(cheapest_observed),
                # Uma representação certificada é viável quando decide ao menos
                # uma vez. Abstenção total não é erro, mas também não é resposta.
                "certified_viable": cheapest_certified is not None,
                # O que o certificado custa a mais por decisão correta, quando
                # ambos existem. É o número que o capítulo precisa defender.
                "certification_premium": None
                if cheapest_certified is None or cheapest_observed is None
                else cheapest_certified["cost_per_correct"] / cheapest_observed["cost_per_correct"],
            }
        )

    OUTPUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    with OUTPUT_JSON.open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "schema_version": 1,
                "join_key": "profile_hash_hex",
                "cost_configurations": len(costs),
                "coverage_records": len(coverage),
                "joined_points": len(points),
                "join_rate": join_rate,
                "escalation_link_constraints": link_cost,
                "cost_orphans": cost_orphans,
                "coverage_orphans": [
                    {
                        "profile_hash_hex": record["profile_hash_hex"],
                        "representation": record["representation"],
                        "workload": record["workload"],
                        "precision_bits": record["precision_bits"],
                        "threshold": record["threshold"],
                    }
                    for record in coverage_orphans
                ],
                "hash_conflicts": conflicts,
                "points": points,
                "fronts": fronts,
                "dominance": dominance,
            },
            handle,
            indent=2,
            ensure_ascii=False,
        )

    fieldnames = [
        "workload",
        "regime",
        "threshold",
        "representation",
        "precision_bits",
        "rounding",
        "certified",
        "cost",
        "opportunities",
        "definite",
        "abstained",
        "unsound_definite",
        "correct_definite",
        "availability",
        "unsound_rate",
        "cost_per_correct",
        "expected_cost_with_escalation",
        "definitive_horizon",
        "profile_hash_hex",
    ]
    with OUTPUT_CSV.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for point in sorted(
            points,
            key=lambda item: (
                item["workload"],
                item["regime"],
                item["threshold"],
                item["cost_per_correct"] if item["cost_per_correct"] is not None else float("inf"),
            ),
        ):
            writer.writerow(point)

    # Uma figura por workload, painéis pelo limiar principal de cada regime.
    for workload, filename in [
        ("logistic-map", "pareto_logistic.svg"),
        ("stable-affine", "pareto_affine.svg"),
    ]:
        panels = []
        for (point_workload, regime, threshold), group in sorted(groups.items()):
            if point_workload != workload:
                continue
            usable = [item for item in group if item["cost_per_correct"] is not None]
            if usable:
                panels.append((f"{regime}, limiar {threshold}", usable))
        if panels:
            svg_scatter(
                ROOT / "results" / filename,
                f"Custo por decisão correta × taxa de decisões incorretas ({workload})",
                panels,
                "constraints por decisão correta",
                "decisões incorretas",
            )
            print(f"results/{filename}")

    print(f"custo: {len(costs)} configurações")
    print(f"cobertura: {len(coverage)} registros")
    print(f"junção: {len(points)} pontos ({join_rate:.1%})")
    if conflicts:
        print(f"CONFLITOS DE HASH: {len(conflicts)}")
        for line in conflicts[:5]:
            print(f"  {line}")
    if cost_orphans:
        print(f"custo sem cobertura: {len(cost_orphans)}")
    if coverage_orphans:
        print(f"cobertura sem custo: {len(coverage_orphans)}")
        seen = set()
        for record in coverage_orphans:
            key = (record["representation"], record["workload"], record["threshold"])
            if key in seen:
                continue
            seen.add(key)
            print(f"  {record['representation']} {record['workload']} limiar {record['threshold']}")
    print(f"\n{OUTPUT_JSON.relative_to(ROOT)}")
    print(f"{OUTPUT_CSV.relative_to(ROOT)}")

    if conflicts:
        print("\nhashes em conflito: dois perfis distintos com a mesma chave", file=sys.stderr)
        return 1
    if join_rate < MINIMUM_JOIN_RATE:
        print(
            f"\njunção de {join_rate:.1%} abaixo do mínimo de {MINIMUM_JOIN_RATE:.0%}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Audit historical data and render figures 1-4 of progress record 0028.

prepare streams the exact JSONL once, checks its hash and predicates, and
exports small derived tables. render consumes only those audited tables.
Neither command overwrites an existing output directory or reruns dynamics.
"""
import argparse
from collections import Counter, defaultdict
import csv
from fractions import Fraction
from functools import lru_cache
import hashlib
import json
import math
from pathlib import Path
import platform
import subprocess
import sys

from qualification_metrics import (
    CERTIFIED_FAMILIES, OUTCOMES, rational,
    summarize_timing, validate_numeric_row,
)

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = ROOT / "experiments/qualification_group_a_v1.json"
COLORS = {"certified_same": "#16856b", "observed_same": "#477ca8",
          "observed_flip": "#cc493d", "abstention": "#e4b34b",
          "reference_unresolved": "#a6adb4"}
LABELS = {"certified_same": "Certificada e concordante", "observed_same": "Concordância observada",
          "observed_flip": "Divergência", "abstention": "Abstenção",
          "reference_unresolved": "Referência inconclusiva"}
NAMES = {"fixed-truncate": "Fixed trunc.", "fixed-rne": "Fixed RNE",
         "fp32": "FP32 restrito", "cbd-local": "CBD local",
         "cbd-sequential": "CBD seq.", "cfd": "CFD",
         "cdad-sequential": "CDAD escada", "cdad-margin-directed": "CDAD margem",
         "cdad-preview-all": "CDAD preview"}


def load(path):
    return json.loads(path.read_text())


def save(path, value):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2, ensure_ascii=False, allow_nan=False)
        stream.write("\n")


def digest(path):
    sha = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            sha.update(block)
    return sha.hexdigest()


def table(path, rows):
    if not rows:
        raise ValueError(f"empty required table: {path.name}")
    with path.open("x", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
        writer.writeheader()
        for row in rows:
            writer.writerow({k: json.dumps(v, ensure_ascii=False) if isinstance(v, (list, dict)) else v
                             for k, v in row.items()})


def method_key(row):
    return row["family"], None if row["family"].startswith("cdad-") else row["precision"]


def name(family, precision):
    return NAMES[family] + (f" {'p' if family in ('fp32', 'cfd') else 'F'}{precision}" if precision is not None else "")


@lru_cache(maxsize=2048)
def expected_profile(workload, parameter, innovation, family, bits, threshold):
    profile = {"schema": 1, "workload": workload, "parameter": parameter,
               "innovation": innovation, "family": family, "bits": bits,
               "threshold": threshold, "operations": "historical-circuit-witness-v1",
               "cfd_corridor": "-24:1" if workload == "affine" else "-16:0"}
    return hashlib.sha256(json.dumps(profile, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def opportunity_key(row):
    return (row["workload"], row["parameter"], row["innovation"], *method_key(row), row["horizon"], row["threshold"])


def expected_opportunities(matrix, horizons):
    keys = set()
    workloads = [("logistic", p, None, matrix["logistic_thresholds"]) for p in matrix["logistic_parameters"]]
    workloads += [("affine", p, c, matrix["affine_thresholds"]) for p, c in matrix["affine_parameters"]]
    for workload, parameter, innovation, thresholds in workloads:
        families = [(f, b) for b in matrix["precision_ladder"]
                    for f in ("fixed-truncate", "fixed-rne", "cbd-local", "cbd-sequential")
                    if not (workload == "affine" and f == "cbd-sequential")]
        families += [("cfd", 24), ("fp32", 24)] + [(f"cdad-{p}", None) for p in ("sequential", "margin-directed", "preview-all")]
        for family, bits in families:
            for horizon in horizons:
                for threshold in thresholds:
                    keys.add((workload, parameter, innovation, family, bits, horizon, threshold))
    return keys


def matches(row, specification):
    return all(row[k] == specification[k] for k in ("workload", "parameter", "threshold"))


def prepare(input_dir, output_dir, config_path, timing_dir=None):
    config = load(config_path)
    manifest = load(input_dir / "manifest.json")
    matrix = load(input_dir / "matrix.json")
    inputs = load(input_dir / "inputs.json")
    if config["schema_version"] != 1 or config["analysis_id"] != "qualification-group-a-v1":
        raise ValueError("unsupported analysis schema")
    if manifest["status"] != "numeric-complete" or manifest["rejected"] != 0:
        raise ValueError("requires a complete run without rejected configurations")
    if matrix["protocol_id"] != config["numeric_protocol"] or manifest["schema_version"] != 1:
        raise ValueError("numeric protocol mismatch")
    if manifest["horizons"] != config["horizons"]:
        raise ValueError("analysis horizons differ from available numeric horizons")
    for filename, key in (("inputs.json", "inputs_hash"), ("matrix.json", "matrix_hash")):
        if digest(input_dir / filename) != manifest[key]:
            raise ValueError(f"source hash mismatch: {filename}")
    metadata = {item["id"]: item for item in inputs}
    if len(metadata) != len(inputs) or len(inputs) != manifest["inputs"]:
        raise ValueError("duplicate inputs or input count mismatch")
    if not any(i["set"] == config["sample_set"] for i in inputs):
        raise ValueError("sample population missing; no historical fallback")
    if rational(metadata[config["trajectory"]["input_id"]]["value"]) != Fraction(1, 10):
        raise ValueError("historical input is not x0=1/10")
    output_dir.mkdir()
    save(output_dir / "started.json", {"status": "running", "input_dir": str(input_dir.resolve())})
    try:
        aggregates = defaultdict(Counter)
        profiles = defaultdict(set)
        margins, history = [], []
        expected = expected_opportunities(matrix, manifest["horizons"])
        previous_input = None
        completed_inputs, seen = set(), set()
        raw_hash = hashlib.sha256()
        count = 0
        margin_methods = {tuple(m) for m in config["margin"]["methods"]}
        with (input_dir / "raw.jsonl").open("rb") as stream:
            for count, line in enumerate(stream, 1):
                raw_hash.update(line)
                row = json.loads(line)
                input_id = row["input_id"]
                if input_id not in metadata or row["input_set"] != metadata[input_id]["set"]:
                    raise ValueError(f"input identity mismatch at line {count}")
                if input_id != previous_input:
                    if previous_input is not None:
                        if seen != expected:
                            raise ValueError(f"incomplete opportunities for {previous_input}")
                        completed_inputs.add(previous_input)
                    if input_id in completed_inputs:
                        raise ValueError("repeated/non-contiguous input block")
                    previous_input, seen = input_id, set()
                identity = opportunity_key(row)
                if identity not in expected or identity in seen:
                    raise ValueError(f"unexpected/duplicate opportunity at line {count}")
                seen.add(identity)
                if row["precision"] not in (16, 32, 48, 64) and row["family"] not in ("fp32", "cfd"):
                    raise ValueError("unsupported precision")
                expected_hash = expected_profile(row["workload"], row["parameter"], row["innovation"],
                                                 row["family"], row["precision"], row["threshold"])
                if row["profile_hash"] != expected_hash:
                    raise ValueError(f"profile hash mismatch at line {count}")
                outcome, lo, hi, tau = validate_numeric_row(row)
                family, bits = method_key(row)
                key = (row["input_set"], row["workload"], row["parameter"], row["innovation"] or "",
                       family, bits, row["horizon"], row["threshold"])
                totals = aggregates[key]
                totals["n"] += 1
                totals[outcome] += 1
                totals["certified_definitive"] += row["certified"] and row["decision"] != "Indeterminate"
                totals["all_abstentions"] += row["decision"] == "Indeterminate"
                totals["resolved"] += row["reference_decision"] is not None
                totals["precision_sum"] += row["precision"]
                totals["evaluations_sum"] += len(row["evaluations"])
                profiles[key].add(row["profile_hash"])
                if input_id == config["trajectory"]["input_id"]:
                    item = {k: row[k] for k in ("input_id", "workload", "parameter", "innovation", "family",
                            "precision", "horizon", "threshold", "decision", "reference_decision", "predicate_flip", "profile_hash")}
                    item["display_center"] = float((lo + hi) / 2)
                    history.append(item)
                if row["input_set"] == config["sample_set"] and matches(row, config["margin"]) and (family, bits) in margin_methods:
                    center = (lo + hi) / 2
                    radius, margin = (hi - lo) / 2, abs(center - tau)
                    margins.append({"source_line": count, "input_id": input_id, "family": family,
                                    "precision": bits, "horizon": row["horizon"], "profile_hash": row["profile_hash"],
                                    "lower": str(lo), "upper": str(hi), "threshold": str(tau),
                                    "margin_exact": str(margin), "radius_exact": str(radius),
                                    "margin": float(margin), "radius": float(radius),
                                    "observed_error": row["center_absolute_error"], "outcome": outcome})
                if count % 200000 == 0:
                    print(f"Validated {count:,} exact records", flush=True)
        if previous_input is None or seen != expected:
            raise ValueError("empty source or incomplete last input")
        completed_inputs.add(previous_input)
        if completed_inputs != set(metadata) or count != manifest["rows"]:
            raise ValueError("source population/row count mismatch")
        if raw_hash.hexdigest() != manifest["raw_hash"]:
            raise ValueError("raw source hash mismatch")
        summary = []
        for key, totals in sorted(aggregates.items()):
            if sum(totals[c] for c in OUTCOMES) != totals["n"]:
                raise ValueError("outcome categories do not partition opportunities")
            item = dict(zip(("input_set", "workload", "parameter", "innovation", "family", "precision", "horizon", "threshold"), key))
            item.update({k: totals[k] for k in ("n", *OUTCOMES, "certified_definitive", "all_abstentions", "resolved")})
            item.update(coverage=totals["certified_definitive"] / totals["n"],
                        mean_precision=totals["precision_sum"] / totals["n"],
                        mean_evaluations=totals["evaluations_sum"] / totals["n"],
                        profile_hashes=sorted(profiles[key]))
            summary.append(item)
        trajectories = []
        trajectory_spec = config["trajectory"]
        with (input_dir / "trajectories.csv").open(newline="") as stream:
            for row in csv.DictReader(stream):
                if row["input_id"] == trajectory_spec["input_id"] and row["workload"] == trajectory_spec["workload"] and row["parameter"] in trajectory_spec["parameters"] and int(row["iteration"]) <= trajectory_spec["maximum_iteration"] and (row["family"], int(row["precision"])) in {tuple(m) for m in trajectory_spec["methods"]}:
                    trajectories.append({**row, "iteration": int(row["iteration"]), "precision": int(row["precision"]),
                                         **{k: float(row[k]) for k in ("center", "lower", "upper", "reference_center", "error", "width")}})
        timing_summary = []
        source_files = {f: digest(input_dir / f) for f in ("manifest.json", "matrix.json", "inputs.json", "trajectories.csv")}
        source_files["raw.jsonl"] = raw_hash.hexdigest()
        if timing_dir:
            tm = load(timing_dir / "manifest.json")
            if tm["status"] != "proof-campaign-complete":
                raise ValueError("incomplete timing campaign")
            buckets = defaultdict(list)
            for row in load(timing_dir / "timing.json"):
                if not row["warmup"] and row["scenario"] == config["timing_scenario"]:
                    buckets[(row["policy"], row["fixed_precision"])].append(row)
            if not buckets:
                raise ValueError("requested timing scenario missing")
            for (policy, bits), rows in sorted(buckets.items()):
                if len(rows) != tm["rounds"] or len({r["round"] for r in rows}) != len(rows):
                    raise ValueError("missing/duplicate timing rounds")
                timing_summary.append({"policy": policy, "precision": bits, **summarize_timing(rows)})
            source_files["timing/manifest.json"] = digest(timing_dir / "manifest.json")
            source_files["timing/timing.json"] = digest(timing_dir / "timing.json")
        save(output_dir / "config.json", config)
        for filename, rows in (("summary", summary), ("margins", margins), ("trajectories", trajectories), ("historical_decisions", history)):
            save(output_dir / f"{filename}.json", rows)
            table(output_dir / f"{filename}.csv", rows)
        save(output_dir / "timing_summary.json", timing_summary)
        if timing_summary:
            table(output_dir / "timing_summary.csv", timing_summary)
        data_hashes = {p.name: digest(p) for p in sorted(output_dir.iterdir()) if p.name != "started.json"}
        files = [Path(__file__), ROOT / "scripts/qualification_metrics.py", ROOT / "scripts/plot_qualification.py", config_path, ROOT / "Cargo.lock"]
        provenance = {str(p.relative_to(ROOT)) if p.is_relative_to(ROOT) else str(p): digest(p) for p in files}
        save(output_dir / "manifest.json", {
            "status": "data-complete", "schema_version": 1, "analysis_id": config["analysis_id"],
            "raw_records_validated": count, "inputs": len(inputs), "input_sets": dict(Counter(i["set"] for i in inputs)),
            "source_directory": str(input_dir.resolve()), "timing_directory": str(timing_dir.resolve()) if timing_dir else None,
            "source_hashes": source_files, "source_numeric_manifest": manifest,
            "data_hashes": data_hashes, "code_hashes": provenance,
            "git_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            "git_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
            "python": platform.python_version(), "platform": platform.platform(),
            "reference_unresolved_rows": sum(s["reference_unresolved"] for s in summary),
            "certified_contradictions": 0, "proofs_executed": 0,
            "validation_scope": "Exact endpoint predicates, consistency, population, profile/source hashes. Not a new proof of numerical inclusion or a cryptographic verification.",
        })
        print(f"Data complete: {count:,} records, {len(summary)} groups", flush=True)
    except Exception as exc:
        save(output_dir / "failure.json", {"status": "failed", "error": str(exc)})
        raise


def quote(text):
    return json.dumps(str(text), ensure_ascii=False)


def data_file(directory, filename, rows):
    with (directory / filename).open("x") as stream:
        for row in rows:
            stream.write(" ".join(format(x, ".17g") if isinstance(x, float) else str(x) for x in row) + "\n")
    return quote(filename)


def panel_layout(title, rows, columns, footnotes, margins="0.10,0.96,0.18,0.90", spacing="0.08,0.12"):
    footer = "\n".join(f"set label {900+i} {quote(note)} at screen 0.06,{0.105-i*0.032:.3f} left font ',10' front" for i, note in enumerate(footnotes))
    return f"{footer}\nset multiplot layout {rows},{columns} margins {margins} spacing {spacing} title {quote(title)} font ',17'\n"


def emit_figure(out, number, body, width=12, height=8.5):
    common = "\n".join(("set encoding utf8", "set border 3 lc rgb '#52606d'", "set tics nomirror",
                        "set grid back lc rgb '#e4e8ed'", "set style fill solid 1.0 noborder")) + "\n"
    # Screen labels would otherwise be painted once per subplot, darkening
    # text and duplicating it in PDF extraction. Render the footer once.
    footer_lines = [line for line in body.splitlines() if line.startswith("set label 9")]
    body = "\n".join(line for line in body.splitlines() if not line.startswith("set label 9"))
    footer = "\n".join((
        "unset label", "unset object", "unset key", "unset title", "unset xlabel", "unset ylabel",
        "unset colorbox", "unset border", "unset grid", "unset tics", "unset logscale",
        "set origin 0,0", "set size 1,0.14", "set lmargin at screen 0.02",
        "set rmargin at screen 0.98", "set bmargin at screen 0.01", "set tmargin at screen 0.14",
        "set xrange [0:1]", "set yrange [0:1]", *footer_lines, "plot NaN notitle",
    ))
    script = ""
    for terminal, suffix in ((f"pdfcairo color noenhanced size {width}in,{height}in font 'Arial,11'", "pdf"),
                             (f"svg size {int(width*110)},{int(height*110)} noenhanced font 'Arial,13'", "svg")):
        script += f"reset\nset terminal {terminal}\nset output '{number:02d}.{suffix}'\n{common}{body}\n{footer}\nunset multiplot\nunset output\n"
    path = out / f"{number:02d}.gp"
    with path.open("x", encoding="utf-8") as stream:
        stream.write(script)
    subprocess.run(["gnuplot", path.name], cwd=out, check=True)


def find_summary(summary, config, specification, family, bits, horizon):
    rows = [r for r in summary if r["input_set"] == config["sample_set"] and matches(r, specification)
            and r["family"] == family and r["precision"] == bits and r["horizon"] == horizon
            and ("innovation" not in specification or r["innovation"] == (specification["innovation"] or ""))]
    if len(rows) != 1:
        raise ValueError(f"missing/ambiguous summary group: {specification}, {family}, {bits}, {horizon}")
    return rows[0]


def plot_trajectories(out, config, trajectories, history):
    spec = config["trajectory"]
    tau = float(rational(spec["threshold"]))
    palette = ["#b35c22", "#4174a5", "#8859a4", "#13816b"]
    body = panel_layout("1. A representação altera a trajetória e o predicado", 2, 1, [
        f"Entrada histórica x0=1/10; limiar={spec['threshold']}. Curvas são visualizações numéricas, não provas de todos os passos.",
        "Marcadores: divergências nos horizontes com registro exato. Faixa clara: enclosure CBD F16.",
        "A comparação não indica falha criptográfica nem descumprimento de IEEE. Fonte: dados históricos auditados.",
    ])
    for panel_index, parameter in enumerate(spec["parameters"]):
        groups = {}
        for family, bits in spec["methods"]:
            values = sorted((r for r in trajectories if r["parameter"] == parameter and r["family"] == family and r["precision"] == bits), key=lambda r: r["iteration"])
            if [r["iteration"] for r in values] != list(range(spec["maximum_iteration"] + 1)):
                raise ValueError("missing or duplicated trajectory points")
            groups[(family, bits)] = values
        reference = next(iter(groups.values()))
        ref_file = data_file(out, f"01-reference-{parameter.replace('/', '-')}.dat", [(r["iteration"], r["reference_center"]) for r in reference])
        plots = []
        for index, ((family, bits), values) in enumerate(groups.items()):
            filename = f"01-{parameter.replace('/', '-')}-{family}-{bits}.dat"
            points = data_file(out, filename, [(r["iteration"], r["center"], r["lower"], r["upper"]) for r in values])
            if family == "cbd-local":
                plots.insert(0, f"{points} using 1:3:4 with filledcurves fc rgb '#dcebe6' title 'Intervalo CBD'")
            plots.append(f"{points} using 1:2 with lines lw 1.5 lc rgb '{palette[index]}' title {quote(name(family,bits))}")
            marked = [r for r in history if r["workload"] == spec["workload"] and r["parameter"] == parameter and r["threshold"] == spec["threshold"] and r["family"] == family and r["precision"] == bits and r["horizon"] <= spec["maximum_iteration"] and r["predicate_flip"] is True]
            if marked:
                markers = data_file(out, filename.replace(".dat", "-flips.dat"), [(r["horizon"], r["display_center"]) for r in marked])
                plots.append(f"{markers} using 1:2 with points pt 7 ps 1.1 lc rgb '{palette[index]}' notitle")
        plots += [f"{ref_file} using 1:2 with lines lw 1.8 dt 2 lc rgb '#202c39' title 'Referência dirigida (ponto médio)'", f"{tau} with lines dt 3 lc rgb '#67717c' notitle"]
        regime = "regime estável" if parameter == "7/2" else "regime caótico"
        legend = "set key at screen 0.52,0.945 center horizontal maxrows 2 font ',9'" if panel_index == 0 else "unset key"
        body += f"set title {quote('r='+str(float(rational(parameter))).replace('.',',')+' - '+regime)}\nset xrange [0:{spec['maximum_iteration']}]\nset yrange [0:1]\nset xtics 10\nset ytics 0.25\nset xlabel 'Iteração'\nset ylabel 'Estado x'\n{legend}\nplot " + ", ".join(plots) + "\n"
    emit_figure(out, 1, body, height=9)


def plot_outcomes(out, config, summary):
    spec = config["outcomes"]
    sample_n = find_summary(summary, config, spec, *spec["methods"][0], config["horizons"][0])["n"]
    body = panel_layout("2. Decisões, divergências e abstenções", 3, 2, [
        f"Logístico r={spec['parameter']}; limiar={spec['threshold']}; {sample_n} entradas aleatórias em cada barra e horizonte.",
        "Certificação e concordância observada são categorias distintas. Referência inconclusiva tem precedência na partição.",
        "F indica bits fracionários; p indica significando. CDAD reúne todas as precisões selecionadas. Fonte: raw.jsonl auditado.",
    ], margins="0.16,0.97,0.17,0.91", spacing="0.20,0.08")
    ticks = ", ".join(f"{quote(name(f,b))} {len(spec['methods'])-i}" for i,(f,b) in enumerate(spec["methods"]))
    for horizon in config["horizons"]:
        category_rows = {c: [] for c in OUTCOMES}
        for index, (family, bits) in enumerate(spec["methods"]):
            row = find_summary(summary, config, spec, family, bits, horizon)
            if row["n"] != sample_n:
                raise ValueError("incomparable outcome populations")
            left, y = 0, len(spec["methods"]) - index
            for category in OUTCOMES:
                right = left + row[category] * 100 / row["n"]
                if right > left:
                    category_rows[category].append([y, (left+right)/2, left, right, y-.35, y+.35])
                left = right
            if not math.isclose(left, 100):
                raise ValueError("stack does not sum to 100 percent")
        plots = []
        for category, points in category_rows.items():
            if points:
                filename = data_file(out, f"02-h{horizon}-{category}.dat", points)
                plots.append(f"{filename} using 2:1:3:4:5:6 with boxxyerrorbars lc rgb '{COLORS[category]}' notitle")
        body += f"set title 'Horizonte {horizon}'\nset xrange [0:100]\nset yrange [0.3:{len(spec['methods'])+.7}]\nset xtics 25\nset ytics ({ticks}) font ',9'\nset xlabel 'Oportunidades (%)'\nunset ylabel\nunset key\nplot " + ", ".join(plots) + "\n"
    body += "unset border\nunset grid\nunset tics\nunset xlabel\nset title 'Como interpretar'\nset xrange [0:1]\nset yrange [0:1]\n"
    for index, category in enumerate(OUTCOMES):
        y = .87-index*.16
        body += f"set object {index+1} rectangle from graph 0.02,{y-.03} to graph 0.09,{y+.03} fc rgb '{COLORS[category]}' front\nset label {index+1} {quote(LABELS[category])} at graph 0.13,{y} left front font ',11'\n"
    body += "plot NaN notitle\n"
    emit_figure(out, 2, body, height=11)


def plot_coverage(out, config, summary):
    spec = config["coverage"]
    body = panel_layout("3. Onde a precisão ainda permite certificar", 2, 3, [
        "Cor e rótulo: decisões definitivas certificadas / todas as entradas aleatórias, em cada perfil e horizonte.",
        "CBD local com F16, F32, F48 e F64. Os controles afins preservam a inicialização histórica documentada.",
        "As linhas são horizontes avaliados, não o instante exato da primeira abstenção. Fonte: raw.jsonl auditado.",
    ], margins="0.08,0.94,0.20,0.88", spacing="0.10,0.12")
    body += "set palette defined (0 '#fff2d9', 50 '#80beb0', 100 '#16685c')\nset cbrange [0:100]\nunset colorbox\nunset grid\nunset key\n"
    xtics = ", ".join(f"'{b}' {i+1}" for i,b in enumerate(spec["precisions"]))
    ytics = ", ".join(f"'{h}' {i+1}" for i,h in enumerate(config["horizons"]))
    for index, panel in enumerate(spec["panels"]):
        points = []
        for y, horizon in enumerate(config["horizons"], 1):
            for x, bits in enumerate(spec["precisions"], 1):
                row = find_summary(summary, config, panel, spec["family"], bits, horizon)
                points.append([x, y, row["coverage"]*100, row["n"], row["certified_definitive"]])
        filename = data_file(out, f"03-panel-{index}.dat", points)
        title = f"Logístico r={panel['parameter']}" if panel["workload"] == "logistic" else f"Afim a={panel['parameter']}, c={panel['innovation']}"
        title += f"\nlimiar={panel['threshold']}"
        body += f"set title {quote(title)}\nset xrange [0.5:4.5]\nset yrange [5.5:0.5]\nset xtics ({xtics})\nset ytics ({ytics})\nset xlabel 'Bits fracionários F'\nset ylabel 'Horizonte'\nplot {filename} using 1:2:3 with image, '' using 1:2:(sprintf('%.1f%%',$3)):($3>55 ? 16777215 : 2107957) with labels tc rgb variable font ',10'\n"
    emit_figure(out, 3, body)


def plot_margins(out, config, margins):
    spec = config["margin"]
    floor = spec["display_floor"]
    body = panel_layout("4. A distância ao limiar e a incerteza não são a mesma coisa", 2, 2, [
        f"Logístico r={spec['parameter']}; limiar={spec['threshold']}; todos os horizontes e entradas aleatórias, sem subamostragem.",
        f"Margem horizontal linear; vertical logarítmica, com piso visual {floor:g}. Decisões calculadas com racionais exatos.",
        "Verde: certificada; amarelo: ambígua; azul/vermelho: concordância/divergência observada. Erro observado não é bound.",
    ], margins="0.10,0.97,0.19,0.89", spacing="0.12,0.12")
    counts = []
    for family, bits in spec["methods"]:
        rows = [r for r in margins if r["family"] == family and r["precision"] == bits]
        if not rows:
            raise ValueError("missing margin population")
        certified = family in CERTIFIED_FAMILIES
        value_key = "radius" if certified else "observed_error"
        clamped = sum(r[value_key] < floor for r in rows)
        counts.append({"family": family, "precision": bits, "points": len(rows), "below_display_floor": clamped})
        plots = []
        for category in OUTCOMES:
            points = [(r["margin"], max(r[value_key], floor)) for r in rows if r["outcome"] == category]
            if points:
                filename = data_file(out, f"04-{family}-{bits}-{category}.dat", points)
                plots.append(f"{filename} using 1:2 with points pt 7 ps 0.23 lc rgb '{COLORS[category]}' notitle")
        title = f"{name(family,bits)} - {len(rows)} oportunidades\n{clamped} valores abaixo do piso visual"
        body += f"set title {quote(title)}\nset xrange [0:0.5]\nset yrange [{floor}:1]\nset logscale y\nset xtics 0.1\nset ytics ('1' 1,'1e-5' 1e-5,'1e-10' 1e-10,'1e-15' 1e-15,'1e-20' 1e-20)\nset xlabel 'Distância do centro ao limiar'\nset ylabel {quote('Raio do enclosure certificado' if certified else 'Erro central observado (não certificado)')}\nunset key\nplot " + ", ".join(plots + ["x with lines dt 2 lw 1.5 lc rgb '#2a3440' notitle"]) + "\n"
    emit_figure(out, 4, body)
    return counts


def report(config, summary, manifest, timing, margin_counts):
    spec = config["outcomes"]
    text = "# Grupo A: decisões certificadas em sistemas dinâmicos\n\n"
    text += "Reanálise dos dados históricos, sem executar novas trajetórias ou provas. Figuras 1–4 do registro 0028; não são comparações executadas com artefatos externos.\n\n"
    text += f"Foram conferidos {manifest['raw_records_validated']:,} registros exatos e os hashes de entrada, matriz, perfis e dados brutos. Referências inconclusivas: {manifest['reference_unresolved_rows']:,} registros, contados separadamente. Nenhuma contradição certificada foi detectada. Isso não substitui demonstração universal de inclusão.\n\n"
    explanations = [
        ("Trajetórias", "A mesma entrada produz a mesma trajetória em formatos diferentes?", "Entrada histórica 1/10, r=3,5 e r=3,9, até 100 passos, limiar 1/2.", "O efeito da representação deve ser separado da validade da prova.", "Curvas são valores para visualização; os marcadores de divergência vêm dos registros exatos nos horizontes avaliados. Não há nova verificação ZK."),
        ("Resultados de decisão", "Quem responde, diverge ou se abstém?", "Amostra aleatória completa por horizonte, r=3,9 e limiar 1/2.", "Certificação e acerto observado não são intercambiáveis; abster-se não é errar.", "Referência inconclusiva tem precedência nas barras; a contagem de contratos definitivos permanece separada nos dados."),
        ("Cobertura por precisão", "Até onde aumentar F recupera decisões?", "CBD local em quatro regimes logísticos e dois controles afins; todos os horizontes.", "A precisão pode ampliar a região certificável sem eliminar o colapso em horizontes longos.", "O mapa mostra cobertura terminal, não sobrevivência contínua nem primeira abstenção exata."),
        ("Margem e incerteza", "Por que a mesma magnitude de erro pode importar ou não?", "Todas as entradas aleatórias e horizontes, CBD F16/F64, fixed RNE F16 e FP32.", "A decisão depende da posição do intervalo em relação ao limiar, não só do erro médio.", "Raio certificado e erro observado têm painéis distintos; eixo vertical logarítmico com piso visual. A diagonal representa y=margem, não um certificado para baselines sem bound."),
    ]
    for number, (title, question, settings, conclusion, limit) in enumerate(explanations, 1):
        text += f"## Figura {number} — {title}\n\nPergunta: {question}\n\nConfiguração: {settings}\n\nConclusão permitida: {conclusion}\n\nLimitação: {limit}\n\n![Figura {number}]({number:02d}.svg)\n\n"
    text += "## Comparação numérica: r=3,9; limiar=1/2\n\n| Método | Horizonte | Oportunidades | Certificadas definitivas | Divergências | Abstenções | Referência inconclusiva |\n|---|---:|---:|---:|---:|---:|---:|\n"
    for horizon in config["horizons"]:
        for family, bits in (("fixed-rne", 16), ("cbd-local", 16), ("cbd-local", 64), ("cdad-sequential", None)):
            r = find_summary(summary, config, spec, family, bits, horizon)
            text += f"| {name(family,bits)} | {horizon} | {r['n']} | {r['certified_definitive']} | {r['observed_flip']} | {r['all_abstentions']} | {r['reference_unresolved']} |\n"
    text += "\nCDAD pode recuperar cobertura frente a F16; isso não demonstra vantagem frente a F64 nem menor tempo. Nenhum tempo longo foi inferido de constraints.\n"
    if timing:
        text += "\n## Correção da classificação da campanha curta\n\nA campanha abaixo já existia; suas provas não foram reexecutadas. Cada contagem se refere a repetições do mesmo cenário boundary-10, não a entradas independentes.\n\n| Política | Precisão inicial | Repetições | Certificadas | Definitivas sem contrato certificado | Flips | Tempo total médio (s) |\n|---|---:|---:|---:|---:|---:|---:|\n"
        for r in timing:
            text += f"| {r['policy']} | {r['precision']} | {r['repeats']} | {r['certified_decisions']} | {r['observed_decisions']} | {r['flips']} | {r['mean_total']:.4f} |\n"
        text += "\nNão há nova inferência de eficiência. Setup permanece incluído no tempo completo; subtraí-lo não simula uma execução comprovada com setup reutilizado.\n"
    text += "\n## Integridade e reprodução\n\nOs arquivos .dat alimentam os scripts .gp sem resultados embutidos. As tabelas derivadas e os hashes ficam no diretório de análise indicado pelo manifesto. Nenhum dado histórico foi sobrescrito. As decisões usam endpoints racionais exatos; doubles são usados somente na visualização.\n\n"
    text += "Limites da validação: comparação dos predicados e disjunção dos enclosures, integridade dos arquivos e população. Sobreposição de intervalos não prova inclusão universal. Os baselines continuam locais e restritos; os papers do registro 0028 não foram reproduzidos nesta etapa.\n"
    text += "\nPisos visuais por painel da figura 4: `" + json.dumps(margin_counts, ensure_ascii=False) + "`.\n"
    return text


def render(analysis_dir, output_dir, pdf_output=None):
    manifest = load(analysis_dir / "manifest.json")
    if manifest["status"] != "data-complete":
        raise ValueError("analysis is not complete")
    for filename, expected in manifest["data_hashes"].items():
        if digest(analysis_dir / filename) != expected:
            raise ValueError(f"derived data hash mismatch: {filename}")
    if pdf_output and pdf_output.exists():
        raise FileExistsError(pdf_output)
    output_dir.mkdir()
    save(output_dir / "started.json", {"status": "running", "analysis_manifest_sha256": digest(analysis_dir / "manifest.json")})
    try:
        config, summary = load(analysis_dir / "config.json"), load(analysis_dir / "summary.json")
        plot_trajectories(output_dir, config, load(analysis_dir / "trajectories.json"), load(analysis_dir / "historical_decisions.json"))
        plot_outcomes(output_dir, config, summary)
        plot_coverage(output_dir, config, summary)
        margin_counts = plot_margins(output_dir, config, load(analysis_dir / "margins.json"))
        with (output_dir / "report.md").open("x", encoding="utf-8") as stream:
            stream.write(report(config, summary, manifest, load(analysis_dir / "timing_summary.json"), margin_counts))
        combined_hash = None
        if pdf_output:
            from pypdf import PdfReader, PdfWriter
            writer = PdfWriter()
            for number in range(1, 5):
                reader = PdfReader(output_dir / f"{number:02d}.pdf")
                if len(reader.pages) != 1:
                    raise ValueError("expected one page per figure")
                writer.append(reader, outline_item=f"Figura {number}")
            writer.add_metadata({"/Title": "Decisões certificadas em sistemas dinâmicos - Grupo A", "/Author": "proof-numeric-zk"})
            pdf_output.parent.mkdir(parents=True, exist_ok=True)
            with pdf_output.open("xb") as stream:
                writer.write(stream)
            if len(PdfReader(pdf_output).pages) != 4:
                raise ValueError("incomplete combined PDF")
            combined_hash = digest(pdf_output)
        save(output_dir / "manifest.json", {"status": "figures-complete", "schema_version": 1,
            "analysis_id": config["analysis_id"], "analysis_directory": str(analysis_dir.resolve()),
            "analysis_manifest_sha256": digest(analysis_dir / "manifest.json"),
            "source_numeric_raw_sha256": manifest["source_hashes"]["raw.jsonl"],
            "script_sha256": digest(Path(__file__)), "gnuplot": subprocess.check_output(["gnuplot", "--version"], text=True).strip(),
            "figures": [1, 2, 3, 4], "margin_display": margin_counts,
            "output_hashes": {p.name: digest(p) for p in sorted(output_dir.iterdir()) if p.name != "started.json"},
            "combined_pdf": str(pdf_output.resolve()) if pdf_output else None, "combined_pdf_sha256": combined_hash,
            "proofs_executed": 0, "new_numeric_experiments": 0})
        print(f"Figures complete: {output_dir}", flush=True)
    except Exception as exc:
        save(output_dir / "failure.json", {"status": "failed", "error": str(exc)})
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prep = commands.add_parser("prepare")
    prep.add_argument("--input-dir", type=Path, required=True)
    prep.add_argument("--output-dir", type=Path, required=True)
    prep.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    prep.add_argument("--timing-dir", type=Path)
    plot = commands.add_parser("render")
    plot.add_argument("--analysis-dir", type=Path, required=True)
    plot.add_argument("--output-dir", type=Path, required=True)
    plot.add_argument("--pdf-output", type=Path)
    args = parser.parse_args()
    # Public MPFR cap is 16,384 bits; parsing those exact dyadics may exceed
    # Python's default decimal digit limit. This bound remains finite.
    if hasattr(sys, "set_int_max_str_digits"):
        sys.set_int_max_str_digits(20000)
    if args.command == "prepare":
        prepare(args.input_dir, args.output_dir, args.config, args.timing_dir)
    else:
        render(args.analysis_dir, args.output_dir, args.pdf_output)


if __name__ == "__main__":
    main()

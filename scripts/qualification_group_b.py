#!/usr/bin/env python3
"""Independent rational audit + pilot figures 5-7. No empirical bound promotion."""
import argparse
from collections import defaultdict
from fractions import Fraction as F
from functools import lru_cache
import hashlib
import json
import math
from pathlib import Path

from qualification_group_a import digest, emit_figure, panel_layout, quote, save, table
from qualification_metrics import interval_decision


def require(condition, message):
    if not condition:
        raise ValueError(message)


def bigint(value):
    sign, limbs = value
    require(type(sign) is int and sign in (-1, 0, 1), "invalid BigInt sign")
    require(all(type(v) is int and 0 <= v < 2**32 for v in limbs), "invalid BigInt limb")
    return sign * sum(v << (32 * i) for i, v in enumerate(limbs))


def rational(value):
    if isinstance(value, str):
        return F(value)
    return F(bigint(value[0]), bigint(value[1]))


def rne(value, bits):
    quotient, remainder = divmod(value * 2**bits, 1)
    quotient = int(quotient)
    return F(quotient + (remainder > F(1, 2) or (remainder == F(1, 2) and quotient % 2)), 2**bits)


@lru_cache(maxsize=32)
def contract_bound(encoded):
    """Independent exact moment/series-tail derivation, not a grid maximum."""
    c = json.loads(encoded)
    nodes, weights = [rational(v) for v in c["nodes"]], [rational(v) for v in c["weights"]]
    require(c["version"] == 1 and len(nodes) in (2, 4, 8, 16) and len(nodes) == len(weights), "bad contract")
    require(all(abs(r) <= 1 for r in nodes) and all(w > 0 for w in weights), "bad constants")
    require(c["format"]["rounding"] == "nearest-ties-to-even", "unsupported rounding")
    require(c["format"]["signed"] is True and c["format"]["overflow"] == "reject", "bad format")
    bits = c["format"]["fractional_bits"]
    require(bits in (16, 32, 48, 64) and c["format"]["integer_bits"] == 8, "bad width")
    require(c["moment_degree"] == min(2*bits+8, 128), "bad moment degree")
    require(rational(c["term_slack"]) == F(2, 2**bits) and rational(c["sum_slack"]) == F(16, 2**bits), "bad slack")
    require(all((x*2**bits).denominator == 1 for x in nodes + weights), "off-grid constants")
    error, power = F(0), F(1, 2)
    powers = [F(1)] * len(nodes)
    for k in range(c["moment_degree"] + 1):
        moment = sum(w*p for w, p in zip(weights, powers))
        exact = F(2, k+1) if k % 2 == 0 else F(0)
        error += power * abs(exact - moment)
        power /= 2
        powers = [p*r for p, r in zip(powers, nodes)]
    error += (2 + sum(weights)) * power * 2
    return error/4, (2*sum(weights)*rational(c["term_slack"])+rational(c["sum_slack"]))/4


def check(c, cert):
    # Rust struct field order is preserved by the JSON reader, hence this is
    # also a differential check of its pre-existing canonical hash format.
    encoded = json.dumps(c, separators=(",", ":"))
    require(hashlib.sha256(encoded.encode()).hexdigest() == cert["contract_hash"], "profile hash mismatch")
    approximation, verification = contract_bound(encoded)
    bits = c["format"]["fractional_bits"]
    x, y, radius = (rational(cert[k]) for k in ("canonical_input", "output", "claimed_radius"))
    require(abs(x) <= 1 and F(1, 4) <= y <= F(3, 4), "domain failure")
    terms = [rational(v) for v in cert["terms"]]
    require(len(terms) == len(c["nodes"]), "term count mismatch")
    for v in terms + [y, radius]:
        require((v*2**bits).denominator == 1 and -256 <= v < 256, "range/grid failure")
    a = 2*y - 1
    total = F(0)
    for node, weight, h in zip(c["nodes"], c["weights"], terms):
        d = 1 + a*rational(node)
        require(d >= F(1, 2) and abs(d*h-a) <= rational(c["term_slack"]), "term relation failure")
        total += rational(weight)*h
    require(abs(total-rne(x, bits)) <= rational(c["sum_slack"]), "sum relation failure")
    input_error = abs(rne(x, bits)-x)/4
    required = approximation + verification + input_error
    require(radius >= required, "insufficient claimed radius")
    return max(F(0), y-radius), min(F(1), y+radius), {
        "approximation": approximation, "verification": verification,
        "input": input_error, "required_radius": required,
    }


def errors(y, ref):
    lo, hi = rational(ref["lower"]), rational(ref["upper"])
    require(lo <= hi, "reference reversed")
    a, b = abs(y-lo), abs(y-hi)
    return (F(0) if lo <= y <= hi else min(a, b)), max(a, b)


def audit(run):
    load = lambda name: json.loads((run / name).read_text())
    manifest = load("manifest.json")
    require(manifest["status"] == "complete-pilot" and manifest["proof_count"] == 0, "incomplete or unsupported run")
    for name in ("matrix.json", "inputs.json", "provenance.json", "contracts.json", "witnesses.json", "decisions.json", "policies.json", "primitives.json", "search.json", "timings.json"):
        require(manifest["files"][name] == digest(run / name), f"digest mismatch: {name}")
    m, data = load("matrix.json"), load("inputs.json")
    contracts = {(c["format"]["fractional_bits"], len(c["nodes"])): c for c in load("contracts.json")}
    require(len(contracts) == 16, "missing/duplicate profiles")
    witnesses = load("witnesses.json")
    cache = {}
    for w in witnesses:
        cert = w["certificate"]
        x = rational(cert["canonical_input"])
        key = w["bits"], w["order"], x
        require(key not in cache, "duplicate witness")
        lo, hi, bounds = check(contracts[key[:2]], cert)
        require(bounds == {k: rational(v) for k, v in w["bound"].items()}, "bound decomposition mismatch")
        cache[key] = (lo, hi, rational(cert["output"]), rational(cert["claimed_radius"]), bounds)
    samples = {s["id"]: s for s in data["samples"]}
    decisions = load("decisions.json")
    require(len(decisions) == len(samples)*16*3, "decision count mismatch")
    groups, seen = defaultdict(list), set()
    for row in decisions:
        key = row["sample_id"], row["bits"], row["order"], rational(row["threshold"])
        require(key not in seen, "duplicate decision")
        seen.add(key)
        sample = samples[key[0]]
        require(row["cohort"] == sample["cohort"], "cohort mismatch")
        states = [cache[(key[1], key[2], rational(x))] for x in sample["inputs"]]
        lo, hi, y = [sum(s[i] for s in states)/4 for i in range(3)]
        require((lo, hi, y) == tuple(rational(row[k]) for k in ("lower", "upper", "output")), "composition mismatch")
        decision = interval_decision(lo, hi, key[3])
        ref_lo, ref_hi = rational(row["reference"]["lower"]), rational(row["reference"]["upper"])
        require(lo <= ref_lo <= ref_hi <= hi, "reference outside bound")
        rd = interval_decision(ref_lo, ref_hi, key[3])
        rd = None if rd == "Indeterminate" else rd == "True"
        require(decision == row["decision"] and rd is row["reference_decision"], "wrong decision")
        require(row["observed_flip"] is (None if rd is None else (y >= key[3]) != rd), "wrong flip")
        require(decision == "Indeterminate" or rd is None or (decision == "True") == rd, "certified contradiction")
        require(errors(y, row["reference"]) == (rational(row["error_lower"]), rational(row["error_upper"])), "wrong error interval")
        groups[(row["cohort"], key[1], key[2], key[3])].append(row)
    coverage = []
    for (cohort, bits, order, threshold), rows in sorted(groups.items()):
        conclusive = [r for r in rows if r["reference_decision"] is not None]
        certified = sum(r["decision"] != "Indeterminate" for r in conclusive)
        coverage.append(dict(cohort=cohort, bits=bits, order=order, threshold=str(threshold),
            opportunities=len(rows), reference_conclusive=len(conclusive), reference_unresolved=len(rows)-len(conclusive),
            certified_correct=certified, ambiguous=sum(r["decision"] == "Indeterminate" for r in conclusive),
            observed_flips=sum(r["observed_flip"] is True for r in rows),
            coverage=certified/len(conclusive) if conclusive else None))
    primitive_rows, method_groups = load("primitives.json"), defaultdict(list)
    refs = {}
    grid = set(map(rational, data["scalar_grid"]))
    for p in primitive_rows:
        require(p["method"] in ("taylor", "pade") and p["counts"]["verification_relation_depth"] is None, "unsupported primitive metadata")
        x = rational(p["input"])
        refs[x] = p["reference"]
        el, eu = errors(rational(p["output"]), p["reference"])
        require((el, eu) == (rational(p["error_lower"]), rational(p["error_upper"])), "primitive error mismatch")
        method_groups[(p["bits"], p["method"], p["order"])].append((eu, None))
    for (bits, order, x), (_, _, y, radius, _) in cache.items():
        if x in grid:
            method_groups[(bits, "gl", order)].append((errors(y, refs[x])[1], radius))
    methods = []
    for (bits, method, order), rows in sorted(method_groups.items()):
        require(len(rows) == len(grid), "incomplete scalar grid")
        methods.append(dict(bits=bits, method=method, order=order, samples=len(rows),
            max_error_upper=float(max(r[0] for r in rows)),
            max_radius=float(max(r[1] for r in rows)) if method == "gl" else None,
            evidence="analytical-checked-relation" if method == "gl" else "observed-grid"))
    search_rows, accepted_count = [], 0
    for row in load("search.json"):
        x = rational(row["input"])
        c = contracts[(16, 4)]
        check(c, row["honest"])
        honest_y = rational(row["honest"]["output"])
        max_error, bound = F(0), F(0)
        require(row["accepted"] and len(row["accepted"])+row["rejected"] == 65, "invalid search coverage")
        seen_offsets = set()
        for cert in row["accepted"]:
            require(rational(cert["canonical_input"]) == x, "search input mismatch")
            _, _, _ = check(c, cert)
            y, radius = rational(cert["output"]), rational(cert["claimed_radius"])
            offset = (y-honest_y)*2**16
            require(offset.denominator == 1 and -32 <= offset <= 32 and offset not in seen_offsets, "invalid search offset")
            seen_offsets.add(offset)
            eu = errors(y, refs[x])[1]
            require(eu <= radius, "accepted witness exceeds bound")
            max_error, bound = max(max_error, eu), max(bound, radius)
            accepted_count += 1
        for depth in m["amplification_depths"]:
            gain = 2**depth
            search_rows.append(dict(input=str(x), depth=depth, gain=gain,
                honest_error_upper=float(gain*errors(honest_y, refs[x])[1]),
                found_error_upper=float(gain*max_error), validated_radius=float(gain*bound),
                accepted=len(row["accepted"]), rejected=row["rejected"]))
    return m, coverage, methods, search_rows, dict(scalar_certificates=len(witnesses),
        composed_decisions=len(decisions), accepted_search_witnesses=accepted_count,
        certified_contradictions=0, reference_unresolved=sum(r["reference_decision"] is None for r in decisions),
        primitive_rows=len(primitive_rows), proof_count=0, timing_claim=False)


def dat(out, name, rows):
    with (out/name).open("x") as f:
        for row in rows:
            f.write(" ".join("NaN" if x is None else format(x, ".17g") for x in row)+"\n")
    return quote(name)


def figures(out, coverage, methods, search):
    body = panel_layout("5. Erro honesto, witness aceito e bound composto", 1, 3, [
        "Pergunta: quanto uma sensibilidade conhecida amplifica o desvio permitido? GL F16, ordem 4.",
        "Após sigmoid: k etapas exatas v <- 2v. Busca limitada a +/-32 ULP; pior caso encontrado, não máximo universal.",
        "Controle linear nosso, não rede ZIP. Erros são limites superiores dirigidos; zero não é desenhado em escala log."], spacing="0.10,0.10")
    for i, x in enumerate(sorted({r["input"] for r in search}, key=F)):
        rows = [r for r in search if r["input"] == x]
        path = dat(out, f"search-{i}.dat", [(r["depth"], r["honest_error_upper"] or None, r["found_error_upper"] or None, r["validated_radius"]) for r in rows])
        positive = [r[k] for r in search for k in ("honest_error_upper", "found_error_upper", "validated_radius") if r[k] > 0]
        ymin, ymax = 10**math.floor(math.log10(min(positive))), 10**math.ceil(math.log10(max(positive)))
        body += f"set title 'x = {x}; {rows[0]['accepted']}/65 aceitos'\nset xlabel 'Etapas lineares k'\nset ylabel 'Erro absoluto / raio'\nset logscale y\nset yrange [{ymin}:{ymax}]\nset format y '%.0e'\nset key top left font ',9'\n"
        body += f"plot {path} u 1:2 w lp lw 2 pt 7 lc rgb '#477ca8' t 'Honesto', '' u 1:3 w lp lw 2 pt 5 lc rgb '#cc493d' t 'Encontrado', '' u 1:4 w lp dt 2 lw 2 lc rgb '#16856b' t 'Bound validado'\n"
    emit_figure(out, 5, body)
    body = panel_layout("6. Mesmo formato; algoritmos e contratos diferentes", 2, 2, [
        "Sigmoid em [-1,1], 17 entradas de malha; Horner fixed-point RNE e relação GL quantizada.",
        "Eixo x: grau Taylor / ordem diagonal Padé / número de nós GL. Parâmetros não são unidades de custo.",
        "Linhas contínuas: erro observado. Tracejada: raio GL validado. Comparadores otimizados ainda pendentes."], margins="0.10,0.96,0.19,0.90", spacing="0.10,0.12")
    for bits in (16, 32, 48, 64):
        paths = {}
        for method in ("taylor", "pade", "gl"):
            rows = sorted((r for r in methods if r["bits"] == bits and r["method"] == method), key=lambda r:r["order"])
            paths[method] = dat(out, f"method-{bits}-{method}.dat", [(r["order"],r["max_error_upper"],r["max_radius"]) for r in rows])
        positive = [r["max_error_upper"] for r in methods] + [r["max_radius"] for r in methods if r["max_radius"]]
        ymin, ymax = 10**math.floor(math.log10(min(positive))), 10**math.ceil(math.log10(max(positive)))
        body += f"set title '8 bits inteiros + {bits} fracionários + sinal'\nset logscale y\nset yrange [{ymin}:{ymax}]\nset format y '%.0e'\nset xlabel 'Parâmetro nominal do método'\nset ylabel 'Máximo na malha / raio'\nset key bottom left font ',9'\n"
        body += f"plot {paths['taylor']} u 1:2 w lp lw 2 pt 7 lc rgb '#477ca8' t 'Taylor', {paths['pade']} u 1:2 w lp lw 2 pt 5 lc rgb '#bc772b' t 'Padé', {paths['gl']} u 1:2 w lp lw 2 pt 9 lc rgb '#16856b' t 'GL honesto', '' u 1:3 w lp dt 2 lw 2 lc rgb '#cc493d' t 'GL bound'\n"
    emit_figure(out, 6, body)
    body = panel_layout("7. Bits, ordem e cobertura da decisão na média de quatro sigmoids", 2, 3, [
        "Cada célula: % de decisões certificadas entre referências conclusivas; conjuntos mantidos separados.",
        "O piloto aleatório é pequeno e não estima cobertura populacional. Igualdade simétrica pode deixar a referência inconclusiva.",
        "A média usa soma racional exata. Mais bits não eliminam necessariamente o piso de erro da quadratura."], margins="0.08,0.94,0.19,0.88", spacing="0.10,0.13")
    for cohort in ("random-pilot", "adversarial"):
        for index, threshold in enumerate(("2/5", "1/2", "3/5")):
            rows = [r for r in coverage if r["cohort"] == cohort and r["threshold"] == threshold]
            points = [(r["bits"]//16, (2,4,8,16).index(r["order"])+1, None if r["coverage"] is None else 100*r["coverage"]) for r in rows]
            path = dat(out, f"coverage-{cohort}-{index}.dat", points)
            label = "Aleatório" if cohort == "random-pilot" else "Adversarial"
            body += f"unset logscale\nset format y '%g'\nset title '{label}; tau={threshold}; ref. {rows[0]['reference_conclusive']}/{rows[0]['opportunities']}'\n"
            body += "set xlabel 'Bits fracionários'\nset ylabel 'Ordem GL'\nset xrange [0.5:4.5]\nset yrange [0.5:4.5]\nset xtics ('16' 1,'32' 2,'48' 3,'64' 4)\nset ytics ('2' 1,'4' 2,'8' 3,'16' 4)\nunset key\nunset colorbox\nunset grid\nset cbrange [0:100]\nset palette defined (0 '#fff4dc',100 '#238872')\n"
            body += f"plot {path} u 1:2:3 w image, '' u 1:2:(sprintf('%.0f%%',$3)) w labels textcolor rgb '#182331' font ',10'\n"
    emit_figure(out, 7, body)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--pdf", type=Path)
    args = parser.parse_args()
    if args.pdf and args.pdf.exists():
        raise FileExistsError(args.pdf)
    m, coverage, methods, search, audit_result = audit(args.run_dir)
    args.output_dir.mkdir(parents=False, exist_ok=False)
    table(args.output_dir/"coverage.csv", coverage)
    table(args.output_dir/"methods.csv", methods)
    table(args.output_dir/"accepted-witnesses.csv", search)
    save(args.output_dir/"audit.json", audit_result)
    figures(args.output_dir, coverage, methods, search)
    if args.pdf:
        from pypdf import PdfWriter
        writer = PdfWriter()
        for number in (5,6,7):
            writer.append(args.output_dir/f"{number:02d}.pdf")
        args.pdf.parent.mkdir(parents=True, exist_ok=True)
        with args.pdf.open("xb") as stream:
            writer.write(stream)
    save(args.output_dir/"manifest.json", {"status":"complete-pilot-figures", "audit":audit_result,
        "source_manifest_sha256":digest(args.run_dir/"manifest.json"),
        "script_sha256":digest(Path(__file__)),
        "group_a_helper_sha256":digest(Path(__file__).with_name("qualification_group_a.py")),
        "files":{p.name:digest(p) for p in sorted(args.output_dir.iterdir()) if p.is_file()}})
    print(json.dumps(audit_result, indent=2))


if __name__ == "__main__":
    main()

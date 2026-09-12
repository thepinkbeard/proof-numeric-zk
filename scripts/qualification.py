#!/usr/bin/env python3
"""Public-input qualification orchestration. No private-input claim.

The verifier accepts an EXTERNAL authorized statement, reconstructs parameters
inside the Rust verifier, and never trusts proof-provided verification keys.
"""
import argparse
import csv
import hashlib
import json
import random
import subprocess
import time
import platform
from functools import lru_cache
from decimal import Decimal, localcontext
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
NOVA = ROOT / "target/release/nova-circuit"
QUAL = ROOT / "target/release/qualification"
LADDER = [16, 32, 48, 64]

def save(path, value):
    with Path(path).open("x") as f:
        json.dump(value, f, indent=2, sort_keys=True)
        f.write("\n")

def load(path):
    return json.loads(Path(path).read_text())

def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()

@lru_cache(maxsize=1)
def environment():
    def file_hash(path):
        h=hashlib.sha256()
        with path.open("rb") as f:
            for chunk in iter(lambda:f.read(1024*1024),b""):h.update(chunk)
        return h.hexdigest()
    return {"os":platform.platform(),"python":platform.python_version(),
            "nova_binary_sha256":file_hash(NOVA),
            "qualification_binary_sha256":file_hash(QUAL),
            "orchestrator_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}

def command(args):
    result = subprocess.run(list(map(str, args)), cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"command failed: {' '.join(map(str,args))}\n{result.stderr[-4000:]}")
    return result.stdout

def validate_statement(s):
    if set(s) != {"schema_version", "x0", "r", "threshold", "horizon", "public_inputs"}:
        raise ValueError("unexpected statement fields")
    if s["schema_version"] != 1 or s["public_inputs"] is not True:
        raise ValueError("unsupported statement")
    if type(s["horizon"]) is not int or not 1 <= s["horizon"] <= 1000:
        raise ValueError("invalid horizon")
    if not 0 <= Fraction(s["x0"]) <= 1 or not 0 <= Fraction(s["r"]) <= 4:
        raise ValueError("input domain")
    if Fraction(s["threshold"]) not in (Fraction(1,4), Fraction(1,2)):
        raise ValueError("qualification proof threshold must be 1/4 or 1/2")

def common(s, bits):
    validate_statement(s)
    return ["--representation", "cdad", "--rounding", "nearest-even", "--k", bits//16,
            "--precision", bits, "--x0", s["x0"], "--r", s["r"],
            "--threshold", s["threshold"], "--steps", s["horizon"]]

def scalar(value):
    # halo2curves serde format is canonical little-endian 32-byte hex.
    raw = bytes.fromhex(value)
    if len(raw) != 32:
        raise ValueError("invalid field encoding")
    return int.from_bytes(raw, "little")

def rne(q):
    n, rem = divmod(q.numerator, q.denominator)
    return n + int(2*rem > q.denominator or (2*rem == q.denominator and n % 2))

def check_boundary(s, bits, artifact):
    """Exact PUBLIC boundary verification, not the legacy truncating gadget."""
    statement = load(artifact / "statement.json")
    k = bits//16
    raw = Fraction(s["x0"]) * 2**bits
    m = rne(raw)
    limbs = [0]*k if m == 2**bits else [(m >> (16*(k-1-i))) & 65535 for i in range(k)]
    expected = [0, *limbs, int(m == 2**bits), int(raw.denominator != 1), 0, bits]
    if list(map(scalar, statement["z0"])) != expected:
        raise ValueError("noncanonical initial state/rounding/radius")
    terminal = list(map(scalar, load(artifact / "terminal.json")))
    if len(terminal) != k+5 or terminal[0] != s["horizon"] or terminal[-1] != bits:
        raise ValueError("wrong terminal horizon/precision")
    if terminal[-2] not in (0, 1, 2):
        raise ValueError("invalid terminal decision")
    return ["False", "True", "Indeterminate"][terminal[-2]]

def prove(s, out, policy="sequential", precision=16):
    validate_statement(s)
    out.mkdir()  # atomic reservation; incomplete run has no bundle.json
    env=environment()
    start = time.perf_counter_ns()
    preview_start = time.perf_counter_ns()
    preview = json.loads(command([QUAL, "select", "--x0",s["x0"],"--r",s["r"],
        "--threshold",s["threshold"],"--steps",s["horizon"],"--policy",policy,"--precision",precision]))
    preview_wall = time.perf_counter_ns()-preview_start
    save(out/"selection.json",preview)
    bits_list = [a["precision"] for a in preview["attempts"]]
    if policy == "preview-all":
        bits_list = [preview["selected_precision"]]
    attempts = []
    for i,bits in enumerate(bits_list):
        artifact = out / f"attempt-{i}"
        tick = time.perf_counter_ns()
        command([NOVA,*common(s,bits),"--compress","--export-proof",artifact,"--output",out/f"metrics-{i}.json"])
        wall = time.perf_counter_ns()-tick
        d = check_boundary(s,bits,artifact)
        attempts.append({"directory":artifact.name,"precision":bits,"decision":d,"generation_wall_ns":wall})
    if attempts[-1]["decision"] != preview["decision"]:
        raise ValueError("off-circuit/circuit decision mismatch")
    bundle = {"schema_version":1,"link_contract":"public-canonical-rne-v1","statement":s,
              "statement_hash":digest(s),"policy":policy,"attempts":attempts,
              "selected_precision":preview["selected_precision"],"decision":preview["decision"],
              "preview_wall_ns":preview_wall,"generation_wall_ns":time.perf_counter_ns()-start,
              "privacy_claim":False,"timings_are_cryptographically_proven":False}
    bundle["environment"]=env
    save(out/"bundle.json",bundle)
    return bundle

def verify(s, directory, report_dir):
    start=time.perf_counter_ns()
    validate_statement(s)
    b = load(directory/"bundle.json")
    if b["schema_version"]!=1 or b["link_contract"]!="public-canonical-rne-v1" or b["statement"] != s or b["statement_hash"] != digest(s):
        raise ValueError("bundle does not match externally authorized statement")
    attempts=b["attempts"]
    if not attempts or len(attempts)>4:
        raise ValueError("invalid attempts")
    precisions=[a["precision"] for a in attempts]
    if any(p not in LADDER for p in precisions) or precisions != sorted(set(precisions)):
        raise ValueError("invalid precision chain")
    if precisions[-1]!=b["selected_precision"]:
        raise ValueError("wrong selected precision")
    # This checks the accepted statement and every supplied proof, not honesty of a timing transcript.
    report_dir.mkdir()
    for i,a in enumerate(attempts):
        if a["directory"]!=f"attempt-{i}":
            raise ValueError("noncanonical artifact path")
        artifact=directory/a["directory"]
        command([NOVA,*common(s,a["precision"]),"--verify-proof",artifact,"--output",report_dir/f"verify-{i}.json"])
        decision=check_boundary(s,a["precision"],artifact)
        if decision!=a["decision"] or (i<len(attempts)-1 and decision!="Indeterminate"):
            raise ValueError("inconsistent retry/decision")
    if attempts[-1]["decision"]!=b["decision"]:
        raise ValueError("forged final decision")
    report={"status":"verified","statement_hash":digest(s),"decision":b["decision"],
            "attempts":len(attempts),"verification_wall_ns":time.perf_counter_ns()-start,
            "privacy_claim":False,"policy_efficiency_verified":False}
    save(report_dir/"verification.json",report)
    return report

def scenarios(out):
    out.mkdir()
    for h in [2,10]:
        save(out/f"historical-{h}.json",{"schema_version":1,"x0":"1/10","r":"39/10","threshold":"1/2","horizon":h,"public_inputs":True})
        # Fixed construction, not selected after inspecting CDAD performance.
        with localcontext() as ctx:
            ctx.prec=100
            y=Decimal("0.5")
            for _ in range(h): y=(1-(1-y).sqrt())/2
            j=int(y*(2**64))+1024
        save(out/f"boundary-{h}.json",{"schema_version":1,"x0":str(Fraction(j,2**64)),"r":"4","threshold":"1/2","horizon":h,"public_inputs":True})

def campaign(statements,out,rounds,scenario=None):
    if rounds < 1: raise ValueError("positive rounds required")
    out.mkdir()
    save(out/"started.json",{"status":"running","environment":environment(),"rounds":rounds,"scenario":scenario})
    # Short proof demonstration includes all fixed precisions plus adaptive policies.
    configs=[("fixed",p) for p in LADDER]+[(p,16) for p in ["sequential","margin-directed","preview-all"]]
    configs += [("baseline-fixed-truncate",32),("baseline-fixed-rne",32),("baseline-cbd",32),("baseline-cbd-sequential",32),("baseline-cfd",24),("baseline-fp32",24)]
    rng=random.Random(20260911)
    records=[]
    paths=[p for p in sorted(statements.glob("*.json")) if scenario is None or p.stem==scenario]
    if not paths:raise ValueError("no authorized scenarios selected")
    for statement_path in paths:
        s=load(statement_path)
        # Independent exact reference is used ONLY for evaluation, never selection.
        ref=Fraction(s["x0"])
        for _ in range(s["horizon"]):ref=Fraction(s["r"])*ref*(1-ref)
        reference_decision="True" if ref>=Fraction(s["threshold"]) else "False"
        for round_id in range(rounds+1):
            order=configs.copy();rng.shuffle(order)
            for policy,bits in order:
                name=f"{statement_path.stem}-round{round_id}-{policy}-{bits}"
                trial_start=time.perf_counter_ns()
                if policy.startswith("baseline-"):
                    folder=out/name;folder.mkdir();family=policy.removeprefix("baseline-")
                    representation="standard" if family.startswith("fixed-") else family
                    args=common(s,32 if bits==24 else bits)
                    args[args.index("--representation")+1]=representation
                    args[args.index("--precision")+1]=bits
                    if bits==24:args[args.index("--k")+1]=1
                    if family=="fixed-truncate":args[args.index("--rounding")+1]="truncate"
                    if family=="cfd":args += ["--exponent-lanes=-16:0","--radius-bits",16]
                    tick=time.perf_counter_ns()
                    command([NOVA,*args,"--compress","--export-proof",folder/"proof","--output",folder/"metrics-0.json"])
                    gen=time.perf_counter_ns()-tick
                    tick=time.perf_counter_ns()
                    command([NOVA,*args,"--verify-proof",folder/"proof","--output",folder/"verify-0.json"])
                    ver=time.perf_counter_ns()-tick
                    code=scalar(load(folder/"proof/terminal.json")[-1])
                    if code not in (0,1,2):raise ValueError("invalid baseline decision")
                    bundle={"generation_wall_ns":gen,"attempts":[{}],"decision":["False","True","Indeterminate"][code]}
                    report={"verification_wall_ns":ver}
                    verifier_metrics=[load(folder/"verify-0.json")]
                else:
                    bundle=prove(s,out/name,policy,bits)
                    report=verify(s,out/name,out/f"{name}-verification")
                    verifier_metrics=[load(out/f"{name}-verification"/f"verify-{i}.json") for i in range(len(bundle["attempts"]))]
                metrics=[load(out/name/f"metrics-{i}.json") for i in range(len(bundle["attempts"]))]
                setup=sum(m["setup_seconds"]+(m["compression_setup_seconds"] or 0) for m in metrics)
                verifier_setup=sum(m["setup_seconds"]+(m["compression_setup_seconds"] or 0) for m in verifier_metrics)
                total=(time.perf_counter_ns()-trial_start)/1e9
                certified_family=policy not in ("baseline-fixed-rne","baseline-fixed-truncate","baseline-fp32")
                flip=bundle["decision"]!="Indeterminate" and bundle["decision"]!=reference_decision
                if certified_family and flip:raise ValueError("certified proof contradicts exact reference")
                records.append({"scenario":statement_path.stem,"round":round_id,"warmup":round_id==0,
                    "policy":policy,"fixed_precision":bits,"decision":bundle["decision"],"attempts":len(metrics),
                    "raw_constraints":sum(m["raw_step_constraints"] for m in metrics),
                    "primary_constraints":sum(m["primary_constraints"] for m in metrics),
                    "complete_wall_seconds":total,"prover_setup_seconds":setup,
                    "verifier_setup_seconds":verifier_setup,"setup_excluded_seconds":total-setup-verifier_setup,
                    "certified_family":certified_family,"reference_decision":reference_decision,"predicate_flip":flip,
                    "generation_wall_seconds":bundle["generation_wall_ns"]/1e9,
                    "verification_wall_seconds":report["verification_wall_ns"]/1e9})
                save(out/f"{name}-record.json",records[-1])
                print(f"completed {name}",flush=True)
    save(out/"timing.json",records)
    save(out/"manifest.json",{"status":"proof-campaign-complete","rounds":rounds,
        "has_30_rounds_per_selected_scenario":rounds==30,"selected_scenarios":[p.stem for p in paths],"records":len(records),"setup_reuse":False,
        "note":"cold standalone setup included in wall time; no amortized prover claim"})

def main():
    p=argparse.ArgumentParser();sub=p.add_subparsers(dest="command",required=True)
    for name in ["prove","verify"]:
        c=sub.add_parser(name);c.add_argument("--statement",type=Path,required=True);c.add_argument("--output-dir",type=Path,required=True)
        if name=="prove":
            c.add_argument("--policy",choices=["fixed","sequential","margin-directed","preview-all"],default="sequential");c.add_argument("--precision",type=int,choices=LADDER,default=16)
        else: c.add_argument("--bundle",type=Path,required=True)
    c=sub.add_parser("scenarios");c.add_argument("--output-dir",type=Path,required=True)
    c=sub.add_parser("campaign");c.add_argument("--statements",type=Path,required=True);c.add_argument("--output-dir",type=Path,required=True);c.add_argument("--rounds",type=int,default=30);c.add_argument("--scenario",choices=["historical-2","historical-10","boundary-2","boundary-10"])
    a=p.parse_args()
    if a.command=="prove":prove(load(a.statement),a.output_dir.resolve(),a.policy,a.precision)
    elif a.command=="verify":verify(load(a.statement),a.bundle.resolve(),a.output_dir.resolve())
    elif a.command=="scenarios":scenarios(a.output_dir)
    else:campaign(a.statements,a.output_dir.resolve(),a.rounds,a.scenario)
if __name__=="__main__":main()

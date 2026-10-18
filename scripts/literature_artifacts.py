#!/usr/bin/env python3
"""Pinned numerical components, never a claim of reproducing a whole ZK/MPC system.

External generators run in disposable directories. Upstream sources are neither
patched nor committed here; their complete revisions are pinned separately.
"""
from contextlib import redirect_stdout
from fractions import Fraction as Q
import hashlib
import io
import json
import math
import os
from pathlib import Path
import runpy
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / "target/literature-artifacts"
P = 21888242871839275222246405745257275088548364400416034343698204186575808495617


def require(ok, message):
    if not ok:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(path, value):
    with Path(path).open("x") as f:
        json.dump(value, f, sort_keys=True, indent=2, allow_nan=False)
        f.write("\n")


def prepare_sources():
    lock = json.loads((ROOT / "experiments/literature_sources_v1.json").read_text())
    CACHE.mkdir(parents=True, exist_ok=True)
    for name, source in lock["sources"].items():
        path = CACHE / name
        if not path.exists():
            subprocess.run(["git", "clone", "--no-checkout", source["url"], str(path)], check=True)
            subprocess.run(["git", "-C", str(path), "checkout", "--detach", source["commit"]], check=True)
        head = subprocess.check_output(["git", "-C", str(path), "rev-parse", "HEAD"], text=True).strip()
        require(head == source["commit"], f"unexpected revision: {name}")
        # Check every consumed code file against Git objects, not merely HEAD.
        roots = {"sridhar": ["scripts", "src"], "zkml": ["src/gadgets/nonlinear"],
                 "nfgen": ["src/NFGen"]}[name]
        names = subprocess.check_output(["git", "-C", str(path), "ls-files", *roots], text=True).splitlines()
        source["consumed_tree_hashes"] = {}
        for filename in names:
            if Path(filename).suffix not in (".py", ".rs", ".nr"):
                continue
            blob = subprocess.check_output(["git", "-C", str(path), "show", f"HEAD:{filename}"])
            require((path / filename).read_bytes() == blob, f"modified upstream source: {filename}")
            source["consumed_tree_hashes"][filename] = hashlib.sha256(blob).hexdigest()
    return lock


def rne(x, bits=16):
    x = Q(x) * 2**bits
    n, r = divmod(x.numerator, x.denominator)
    return n + (2*r > x.denominator or (2*r == x.denominator and n % 2 != 0))


def mpq(x):
    import mpmath as mp
    x = Q(x)
    return mp.mpf(x.numerator) / x.denominator


def native_sridhar(method, inputs, bits=16):
    """Unmodified generator, with its random stream replaced by exact frozen inputs.

    This is explicitly an input adapter, not the original random experiment.
    Constants/orders are those of final-art/run_all.py for F16.
    """
    import mpmath as mp
    import toml
    require(bits == 16, "native campaign currently freezes F16")
    spec = {"poly": ("poly_gen.py", "poly_approx", [5, bits], "poly_wits"),
            "pade": ("pade_gen.py", "pade_approx", [3, bits], "pade_wits"),
            "gl": ("gl_gen.py", "gl_quad", [4, bits], "gl_wits"),
            "pwl": ("pwl_gen.py", "pwl", [0, 1, 3, bits], "pwl_wits")}[method]
    script, module, params, key = spec
    source = CACHE / "sridhar/scripts" / script
    previous_cwd, previous_argv, previous_rand = Path.cwd(), sys.argv[:], mp.rand
    values = iter(inputs)
    try:
        with tempfile.TemporaryDirectory(prefix="literature-sridhar-") as scratch:
            scratch = Path(scratch)
            (scratch / "src" / module).mkdir(parents=True)
            (scratch / "Prover.toml").touch()
            os.chdir(scratch)
            sys.argv = [str(source), "sigmoid", str(len(inputs)), *map(str, params)]
            mp.rand = lambda: mpq(next(values))
            with redirect_stdout(io.StringIO()):
                state = runpy.run_path(str(source), run_name="__main__")
            witnesses = toml.loads((scratch / "Prover.toml").read_text())[key]
            constants_source = (scratch / "src" / module / "constants.nr").read_text()
    finally:
        mp.rand, sys.argv = previous_rand, previous_argv
        os.chdir(previous_cwd)
    fields = {"poly": ["coeffs"], "pade": ["num", "denom"],
              "gl": ["quantized_roots", "quantized_weights"],
              "pwl": ["start_points", "linear_coeffs", "piece_size_quantized"]}[method]
    constants = {name: state[name] for name in fields}
    # JSON makes tuples/lists uniform; all field constants remain exact integers.
    constants = json.loads(json.dumps(constants))
    require(len(witnesses) == len(inputs), "incomplete generator output")
    for x, w in zip(inputs, witnesses):
        inp = w["inp_struct"] if method == "gl" else w
        require(int(inp["x"]) == rne(x, bits), "input adapter changed quantization")
        expected = int(mp.nint((1 / (1 + mp.exp(-mpq(x)))) * 2**bits))
        require(int(inp["y"]) == expected, "honest generator output mismatch")
    return {"method": method, "bits": bits, "constants": constants,
            "constants_source": constants_source, "witnesses": witnesses,
            "generator_status": "official-component-executed",
            "relation_status": "local-field-translation-not-Noir-executed"}


def accept_sridhar(profile, w):
    """Full active sigmoid branch translated from pinned Noir, INCLUDING modular arithmetic.

    No extra range constraints are silently inserted into the paper's relation.
    Its cryptographic soundness is NOT assessed by this numerical checker.
    """
    method, s, c = profile["method"], 2**profile["bits"], profile["constants"]
    def small(error, limit):
        return 0 <= error % P < limit
    if method == "gl":
        v, h = w["inp_struct"], w["wit_struct"]
        x, y = int(v["x"]), int(v["y"])
        r, weights = list(map(int, c["quantized_roots"])), list(map(int, c["quantized_weights"]))
        mult, inv = list(map(int, h["mult_terms"])), list(map(int, h["denom_inverses"]))
        return (len(mult) == len(inv) == len(r) and 0 <= y < s
                and all(small(m*s-y*t+4*s, 8*s) for m,t in zip(mult,r))
                and all(small((2*y-s)*s-(2*m-t+s)*z+4*s,8*s) for m,t,z in zip(mult,r,inv))
                and small(x*s-sum(a*b for a,b in zip(weights,inv))+4*s,8*s))
    x,y = int(w["x"]),int(w["y"])
    if method == "pwl":
        i = int(w["segment_index"])
        if not 0 <= i < len(c["start_points"]):
            return False
        start = int(c["start_points"][i]); end = start + int(c["piece_size_quantized"])
        a,b = map(int,c["linear_coeffs"][i])
        return start % P < (x+1) % P and x % P < (end+1) % P and small(y*s-a*x-b*s+16*s,32*s)
    powers = list(map(int,w["intermediates"]))
    coeffs = c["coeffs"] if method == "poly" else c["num"]
    if len(powers) != len(coeffs):
        return False
    tolerance = 4 if method == "poly" else 2
    if not all(small((x*s if j == 1 else powers[j-1]*x)-powers[j]*s+tolerance*s,2*tolerance*s)
               for j in range(1,len(powers))):
        return False
    # Upstream DOES NOT check intermediates[0] == SCALE; preserve that fact.
    numerator = sum(a*int(b) for a,b in zip(powers,coeffs))
    if method == "poly":
        return small(y*s-numerator+128*s,256*s)
    denominator = sum(a*int(b) for a,b in zip(powers,c["denom"]))
    d = int(w["denom_rescaled"])
    return small(denominator-d*s+2*s,4*s) and small(y*d-numerator+4*s,8*s)


def native_zkml(bits=16):
    """Compile the EXACT upstream generate_map body with only std dependencies.

    Parameters cover [-1,1]. The upstream reserved first row lies at -1 and is
    outside the comparison's [0,1] inputs; no endpoint clipping is performed.
    """
    source = (CACHE / "zkml/src/gadgets/nonlinear/logistic.rs").read_text()
    start = source.index("fn generate_map(")
    pos = source.index("{", start); depth = 1; end = pos+1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    body = source[start:end]
    scale = 2**bits
    wrapper = "use std::collections::HashMap;\nstruct Official;\nimpl Official {\n"+body+"\n}\n"
    wrapper += f"fn main() {{ let m = Official::generate_map({scale}, -{scale}, {2*scale+1}); for x in 0..={scale} {{ println!(\"{{}}\", m[&(x+{scale})]); }} }}\n"
    with tempfile.TemporaryDirectory(prefix="literature-zkml-") as d:
        d=Path(d); (d/"map.rs").write_text(wrapper)
        subprocess.run(["rustc","-O",str(d/"map.rs"),"-o",str(d/"map")],check=True)
        values=list(map(int,subprocess.check_output([str(d/"map")],text=True).splitlines()))
    require(len(values)==scale+1,"incomplete native lookup")
    for x,y in enumerate(values):
        # Independent Python port. Nonnegative values => Rust round is floor(z+.5).
        expected=math.floor(scale/(1+math.exp(-x/scale))+.5)
        require(y==expected, f"Rust/Python lookup difference at {x}")
    return values, hashlib.sha256(body.encode()).hexdigest()


def native_nfgen(degrees=(3,5,7,9), bits=16):
    """Official SampledPolyFit + prediction; NOT a run of an MPC backend.

    Pick minimum stored coefficients among feasible candidates on the generator's
    calibration grid. This selector is OUR adaptation, not NFGen's MPC profiler.
    """
    import numpy as np
    import sympy as sp
    sys.path.insert(0,str(CACHE/"nfgen/src"))
    import NFGen.main as generator
    import NFGen.funcs as nf
    np.random.seed(20260918)
    func=lambda x:1/(1+sp.exp(-x))
    candidates=[]
    for degree in degrees:
        coeff,intervals,scales=generator.sampled_poly_fit(
            0.,1.,degree,1e-3,func,bits,2*bits,ms=1000,zero_mask=1e-6,
            plain_method="Cheby")
        breaks,c,s=nf.result_orgnize(intervals,coeff,scales)
        c=np.asarray(c).reshape(len(breaks)-1,-1)
        s=np.asarray(s).reshape(c.shape)
        xs=np.linspace(0.,1.,1001)[:,None]
        prediction=nf.piece_prediction(xs.copy(),c,np.array(breaks),s,bits,2*bits)
        expected=1/(1+np.exp(-xs[:,0]))
        error=float(np.max(np.abs(prediction-expected)/expected))
        candidates.append(dict(degree=degree,breaks=list(breaks),coefficients=c.tolist(),scales=s.tolist(),
                               calibration_relative_error=error,feasible=error<=1e-3))
    eligible=[c for c in candidates if c["feasible"]]
    require(eligible,"no feasible NFGen candidate")
    chosen=min(eligible,key=lambda c:(len(c["coefficients"])*len(c["coefficients"][0]),c["degree"]))
    xs=np.arange(2**bits+1,dtype=float)[:,None]/2**bits
    predictions=nf.piece_prediction(xs.copy(),np.array(chosen["coefficients"]),np.array(chosen["breaks"]),
                                     np.array(chosen["scales"]),bits,2*bits)
    require(np.isfinite(predictions).all(),"NFGen nonfinite output")
    # Decimal emulator, not binary Q16.16. Serialize the actual binary64 values exactly.
    return candidates, chosen, [str(Q(float(y))) for y in predictions]


def noir_check(profile, witnesses):
    """Execute original Noir relation in an isolated entrypoint. NOT a ZK proof.

    Only NUM_INPUTS and the caller are changed; relation source is byte-identical.
    Failed execution is returned, never reinterpreted as witness rejection unless
    the compiler/runtime actually reports an assertion failure.
    """
    import toml
    method=profile["method"]
    module, typ, function = {
        "gl":("gl_quad","GLWitness<constants::FUNC_TYPE>","check_gl_quad"),
        "poly":("poly_approx","PolyWitness","check_poly_approx"),
        "pade":("pade_approx","PadeWitness","check_pade_approx"),
        "pwl":("pwl","PWLWitness","check_pwl_approx"),
    }[method]
    nargo=CACHE/"noir-beta21/nargo"
    require(nargo.is_file(),"install pinned nargo beta.21 for differential validation")
    version=subprocess.check_output([str(nargo),"--version"],text=True)
    require("nargo version = 1.0.0-beta.21\n" in version,"unexpected Noir version")
    with tempfile.TemporaryDirectory(prefix="literature-noir-") as temp:
        d=Path(temp)
        shutil.copytree(CACHE/"sridhar/src"/module,d/"src"/module)
        constants=re.sub(r"NUM_INPUTS: u32 = \d+",f"NUM_INPUTS: u32 = {len(witnesses)}",profile["constants_source"])
        (d/"src"/module/"constants.nr").write_text(constants)
        (d/"Nargo.toml").write_text('[package]\nname = "component_check"\ntype = "bin"\nauthors = [""]\ncompiler-version = ">=1.0.0-beta.6"\n[dependencies]\n')
        (d/"src/main.nr").write_text(f"mod {module};\nuse {module}::constants;\nfn main(wits: [{module}::{typ}; {len(witnesses)}]) {{ for w in wits {{ {module}::{function}(w); }} }}\n")
        (d/"Prover.toml").write_text(toml.dumps(dict(wits=witnesses)))
        p=subprocess.run([str(nargo),"execute"],cwd=d,text=True,capture_output=True,timeout=120)
    output=(p.stdout+p.stderr).replace(str(d),"<scratch>")
    if p.returncode==0:
        require("Circuit witness successfully solved" in output,"unrecognized Noir success")
        return dict(accepted=True,exit_code=p.returncode,output=output)
    require("Assertion failed" in output or "assertion failed" in output or "Failed constraint" in output,
            "Noir compilation/runtime failure, not numerical rejection: "+output[-3000:])
    return dict(accepted=False,exit_code=p.returncode,output=output)

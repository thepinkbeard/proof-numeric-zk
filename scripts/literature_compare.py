#!/usr/bin/env python3
"""Reproducible paired, OFF-CIRCUIT comparison of pinned literature components.

Certifications refer to the declared numerical relation, not a proof execution.
An exhaustive table certificate covers every F16 input, not just this dataset.
"""
import argparse
import csv
from collections import defaultdict
from copy import deepcopy
from fractions import Fraction as Q
import hashlib
import json
import platform
import subprocess
import sys
import time
from pathlib import Path

from literature_artifacts import (ROOT, P, accept_sridhar, digest, mpq, native_nfgen,
                                  native_sridhar, native_zkml, noir_check, prepare_sources, require, rne, save)
from qualification_group_b import contract_bound, rational


class Bridge:
    def __init__(self):
        self.p = subprocess.Popen([str(ROOT/"target/release/literature-bridge")],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    def call(self, command, **kwargs):
        self.p.stdin.write(json.dumps(dict(command=command, **kwargs))+"\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        require(line, "MPFR bridge failed")
        return json.loads(line)
    def refs(self, xs, bits):
        result=[]
        for i in range(0,len(xs),1024):
            result.extend(self.call("reference", inputs=list(map(str,xs[i:i+1024])), bits=bits))
        return result
    def close(self):
        self.p.stdin.close(); code=self.p.wait();self.p.stdout.close();require(code==0,"bridge failed")


def hash_json(x):
    return hashlib.sha256(json.dumps(x,sort_keys=True,separators=(",",":"),allow_nan=False).encode()).hexdigest()


def splitmix(state):
    mask=2**64-1
    while True:
        state=(state+0x9e3779b97f4a7c15)&mask; z=state
        z=((z^(z>>30))*0xbf58476d1ce4e5b9)&mask
        z=((z^(z>>27))*0x94d049bb133111eb)&mask
        yield (z^(z>>31))&mask


def make_inputs(m):
    import mpmath as mp
    mp.mp.dps=100
    samples=[]
    for seed in m["seeds"]:
        gen=splitmix(seed)
        for i in range(m["vectors_per_seed"]):
            samples.append(dict(id=f"seed-{seed}-{i}",cohort="random",seed=seed,
                                xs=[str(Q(next(gen),2**64)) for _ in range(4)]))
    for t in map(Q,m["thresholds"]):
        z=mp.log(mpq(t)/(1-mpq(t)))
        center=Q(int(mp.nint(z*2**64)),2**64)
        for bits in (24,48):
            for sign in (-1,1):
                x=center+sign*Q(1,2**bits)
                samples.append(dict(id=f"boundary-{t}-{bits}-{sign}",cohort="adversarial",seed=None,xs=[str(x)]*4))
    for x in ("0","1"):
        samples.append(dict(id="endpoint-"+x,cohort="adversarial",seed=None,xs=[x]*4))
    grid=list(map(str,(Q(i,m["grid_intervals"]) for i in range(m["grid_intervals"]+1))))
    xs=sorted(set(map(Q,grid))|{Q(x) for s in samples for x in s["xs"]})
    return dict(generator="splitmix64-v1",samples=samples,grid=grid,scalars=list(map(str,xs)))


def interval_decision(lo,hi,t):
    return True if lo>=t else False if hi<t else None


def error_bounds(y,ref):
    lo,hi=map(Q,ref)
    return (Q(0) if lo<=y<=hi else min(abs(y-lo),abs(y-hi))),max(abs(y-lo),abs(y-hi))


def table_bound(values,refs,bits=16):
    """Every grid point checked with directed MPFR; sigma is globally 1/4 Lipschitz.

    RNE input error <= 1/(2S). No independence assumption or observed-grid bound.
    refs must contain ALL S+1 points, in order. Table is a frozen public function.
    """
    require(len(values)==len(refs)==2**bits+1,"exhaustive certificate missing rows")
    error=max(error_bounds(Q(y),ref)[1] for y,ref in zip(values,refs))
    return error+Q(1,8*2**bits)


def own_check(c,w,x):
    require(w["profile_hash"]==hash_json(c),"our profile mismatch")
    require(Q(w["input"])==x and 0<=x<=1,"canonical input/domain mismatch")
    bits=c["format"]["fractional_bits"]; s=2**bits
    y,rad=Q(w["output"]),Q(w["radius"])
    require(Q(1,4)<=y<=Q(3,4) and rad>=0,"our output domain/radius")
    terms=list(map(Q,w["terms"])); nodes=list(map(rational,c["nodes"])); weights=list(map(rational,c["weights"]))
    require(len(terms)==len(nodes),"our witness dimension")
    require(all((v*s).denominator==1 and -256<=v<256 for v in terms+[y,rad]),"our grid/range")
    a=2*y-1
    require(all(abs((1+a*r)*h-a)<=rational(c["term_slack"]) for r,h in zip(nodes,terms)),"our term relation")
    require(abs(sum(w*h for w,h in zip(weights,terms))-Q(rne(x,bits),s))<=rational(c["sum_slack"]),"our sum relation")
    ap,ver=contract_bound(json.dumps(c,separators=(",",":")))
    required=ap+ver+abs(Q(rne(x,bits),s)-x)/4
    require(rad>=required,"our understated bound")
    return max(Q(0),y-rad),min(Q(1),y+rad)


def own_witness(c,x):
    import mpmath as mp
    mp.mp.dps=100
    bits=c["format"]["fractional_bits"]; s=2**bits
    nodes=list(map(rational,c["nodes"])); weights=list(map(rational,c["weights"]))
    rs=list(map(mpq,nodes)); ws=list(map(mpq,weights)); xq=Q(rne(x,bits),s)
    evaluations=0
    def relation(y):
        nonlocal evaluations
        evaluations+=1
        a=2*y-1
        return sum(w*a/(1+a*r) for w,r in zip(ws,rs))-mpq(xq)
    root=mp.findroot(relation,(mp.mpf("0.5"),mp.mpf("0.74")),tol=mp.mpf(2)**(-bits-40),maxsteps=100)
    y=Q(int(mp.nint(root*s)),s); a=2*y-1
    terms=[Q(rne(a/(1+a*r),bits),s) for r in nodes]
    ap,ver=contract_bound(json.dumps(c,separators=(",",":")))
    needed=(ap+ver+abs(xq-x)/4)*s
    radius=Q(-(-needed.numerator//needed.denominator),s)
    w=dict(input=str(x),output=str(y),radius=str(radius),terms=list(map(str,terms)),
           profile_hash=hash_json(c),relation_evaluations=evaluations)
    own_check(c,w,x)
    return w


def make_row(profile,x,y,ref,accepted=True,enclosure=None,**extra):
    el,eu=error_bounds(y,ref)
    return dict(profile=profile,input=str(x),output=str(y),accepted=accepted,
                lower=None if enclosure is None else str(enclosure[0]),
                upper=None if enclosure is None else str(enclosure[1]),
                error_lower=str(el),error_upper=str(eu),**extra)


def decisions(inputs,rows,refs,thresholds,bridge=None):
    groups=defaultdict(dict)
    for r in rows: groups[r["profile"]][r["input"]]=r
    results=[]
    for profile,values in sorted(groups.items()):
        for sample in inputs["samples"]:
            parts=[values[x] for x in sample["xs"]]
            y=sum(Q(r["output"]) for r in parts)/4
            lo=sum(Q(refs[x][0]) for x in sample["xs"])/4
            hi=sum(Q(refs[x][1]) for x in sample["xs"])/4
            accepted=all(r["accepted"] for r in parts)
            certified=accepted and all(r["lower"] is not None for r in parts)
            lower=sum(Q(r["lower"]) for r in parts)/4 if certified else None
            upper=sum(Q(r["upper"]) for r in parts)/4 if certified else None
            for threshold in map(Q,thresholds):
                rd=interval_decision(lo,hi,threshold)
                if rd is None and bridge is not None:
                    for bits in (1024,2048,4096,8192,16384):
                        rr=bridge.refs(list(map(Q,sample["xs"])),bits)
                        lo=sum(Q(r[0]) for r in rr)/4; hi=sum(Q(r[1]) for r in rr)/4
                        rd=interval_decision(lo,hi,threshold)
                        if rd is not None: break
                cd=interval_decision(lower,upper,threshold) if certified else None
                if cd is not None and rd is not None: require(cd==rd,"CERTIFIED CONTRADICTION: abort campaign")
                if certified: require(lower<=lo<=hi<=upper,"reference escapes compositional contract")
                results.append(dict(profile=profile,sample_id=sample["id"],cohort=sample["cohort"],seed=sample["seed"],
                    threshold=str(threshold),output=str(y),reference_lower=str(lo),reference_upper=str(hi),
                    lower=None if lower is None else str(lower),upper=None if upper is None else str(upper),
                    accepted=accepted,reference_decision=rd,observed_decision=y>=threshold,
                    observed_flip=None if rd is None else (y>=threshold)!=rd,contract_available=certified,
                    certified_decision=cd))
    return results


def policies(decision_rows):
    grouped=defaultdict(dict)
    for r in decision_rows:
        if r["profile"].startswith("ours-f"):
            grouped[(r["sample_id"],r["threshold"])][int(r["profile"][6:])]=r
    out=[]; ladder=[16,32,48,64]
    for (sample,threshold),rows in sorted(grouped.items()):
        for policy in ["fixed16","fixed64","sequential","margin","preview-all"]:
            visited=[]; idx=3 if policy=="fixed64" else 0
            while True:
                f=ladder[idx]; visited.append(f); r=rows[f]
                if policy=="preview-all":
                    visited=ladder[:]
                    f=next((b for b in ladder if rows[b]["certified_decision"] is not None),64); r=rows[f]
                    break
                if policy.startswith("fixed") or r["certified_decision"] is not None or idx==3: break
                jump=1
                if policy=="margin":
                    margin=abs(Q(r["output"])-Q(threshold))
                    radius=max(Q(r["output"])-Q(r["lower"]),Q(r["upper"])-Q(r["output"]))
                    if margin==0: jump=3-idx
                    else:
                        desired=f
                        while radius>=margin and desired<64: radius/=2; desired+=1
                        jump=max(1,next((j for j in range(1,4-idx) if ladder[idx+j]>=desired),3-idx))
                idx=min(3,idx+jump)
            out.append(dict(sample_id=sample,threshold=threshold,cohort=r["cohort"],policy=policy,
                            selected_bits=f,evaluated_bits=visited,certified_decision=r["certified_decision"],
                            reference_decision=r["reference_decision"],cost_kind="evaluation-count-replay-not-proving-time"))
    return out


def searches(profiles,inputs,refs,ulps):
    """Limited constructive search; regenerate GL intermediates, then check ALL local field relations."""
    import mpmath as mp
    results=[]
    for profile in profiles:
        for x in [Q(0),Q(1,4),Q(1,2),Q(3,4),Q(1)]:
            index=inputs.index(x); original=profile["witnesses"][index]
            method=profile["method"]; s=2**profile["bits"]
            inp=original["inp_struct"] if method=="gl" else original
            original_y=int(inp["y"]); found=[]
            for delta in range(-ulps,ulps+1):
                w=deepcopy(original); y=original_y+delta
                if method=="gl":
                    w["inp_struct"]["y"]=str(y)
                    roots=profile["constants"]["quantized_roots"]
                    rr=[Q(int(r) if int(r)<P//2 else int(r)-P,s) for r in roots]
                    yy=Q(y,s); a=2*yy-1
                    if any(1+a*r==0 for r in rr): continue
                    w["wit_struct"]["mult_terms"]=[str(rne(yy*r)%P) for r in rr]
                    w["wit_struct"]["denom_inverses"]=[str(rne(a/(1+a*r))%P) for r in rr]
                else: w["y"]=str(y%P)
                if accept_sridhar(profile,w): found.append((error_bounds(Q(y,s),refs[str(x)])[1],delta,w))
            require(found,"search failed to find even one admissible witness")
            error,delta,w=max(found,key=lambda t:t[0])
            results.append(dict(profile="sridhar-"+method,input=str(x),search_trials=2*ulps+1,
                accepted_count=len(found),honest_error=str(error_bounds(Q(original_y,s),refs[str(x)])[1]),
                largest_found_error=str(error),output_delta_ulps=delta,witness=w,
                global_bound=None,scope="local-field-translation; finite-search-not-universal-maximum"))
    return results


def generate(output,matrix):
    require(not output.exists(),"refusing to overwrite a previous run")
    output.mkdir(parents=True)
    save(output/"started.json",dict(status="running",proof_count=0))
    started=time.perf_counter(); bridge=Bridge()
    try:
        m=json.loads(matrix.read_text())
        require(m==json.loads((ROOT/"experiments/literature_comparison_v1.json").read_text()),"unsupported matrix")
        lock=prepare_sources(); save(output/"sources.json",lock); save(output/"matrix.json",m)
        data=make_inputs(m); save(output/"inputs.json",data); xs=list(map(Q,data["scalars"]))
        print(f"Reference: {len(xs)} scalar inputs",flush=True)
        low=bridge.refs(xs,m["reference_bits"]); high=bridge.refs(xs,m["confirmation_bits"])
        for l,h in zip(low,high): require(Q(l[0])<=Q(h[0])<=Q(h[1])<=Q(l[1]),"MPFR confirmation failed")
        refs=dict(zip(data["scalars"],high)); save(output/"references.json",refs)
        rows=[]; profiles=[]; timings={}; metadata={}; differential=[]
        for method in ["gl","poly","pade","pwl"]:
            print("Official generator:",method,flush=True); t=time.perf_counter()
            p=native_sridhar(method,xs); profiles.append(p)
            accepted=[w for w in p["witnesses"] if accept_sridhar(p,w)]
            rejected=[w for w in p["witnesses"] if not accept_sridhar(p,w)]
            check=noir_check(p,accepted)
            require(check["accepted"],"Noir disagrees with accepted honest batch")
            differential.append(dict(method=method,case="honest-accepted-batch",count=len(accepted),result=check))
            for w in rejected:
                check=noir_check(p,[w]); require(not check["accepted"],"Noir disagrees with rejected witness")
                differential.append(dict(method=method,case="honest-rejection",count=1,result=check))
            mutated=deepcopy(p["witnesses"][0]);v=mutated["inp_struct"] if method=="gl" else mutated
            v["y"]=str(4*65536)
            check=noir_check(p,[mutated]);require(not check["accepted"] and not accept_sridhar(p,mutated),"negative-control failure")
            differential.append(dict(method=method,case="altered-output-negative-control",count=1,result=check))
            p["relation_status"]="official-Noir-branch-executed-and-local-checker-compared"
            name="sridhar-"+method
            metadata[name]=dict(source="sridhar",component=p["generator_status"],relation=p["relation_status"],
                bounds="not-established-for-entire-field-relation",fractional_bits=16,
                stored_constant_slots={"gl":8,"poly":6,"pade":8,"pwl":24}[method])
            for x,w in zip(xs,p["witnesses"]):
                v=w["inp_struct"] if method=="gl" else w
                rows.append(make_row(name,x,Q(int(v["y"]),65536),refs[str(x)],accept_sridhar(p,w)))
            timings[name]=time.perf_counter()-t
        save(output/"sridhar.json",profiles)
        print("Native ZKML table and NFGen candidates",flush=True); t=time.perf_counter()
        zk,body_hash=native_zkml(); zvalues=[str(Q(y,65536)) for y in zk]
        candidates,chosen,nvalues=native_nfgen(m["nfgen_degrees"])
        timings["native-table-preparation"]=time.perf_counter()-t
        save(output/"nfgen-candidates.json",dict(candidates=candidates,selected=chosen,
            selector="our-minimum-coefficient-count; not the MPC profiler",semantics="official decimal/float emulator"))
        print("Exhaustive finite-table certificates: 65,537 points",flush=True)
        gridrefs=bridge.refs([Q(i,65536) for i in range(65537)],256)
        save(output/"exhaustive-reference.json",gridrefs)
        tables={"zkml-lookup":zvalues,"nfgen-emulator":nvalues}; save(output/"tables.json",tables)
        bounds={name:str(table_bound(values,gridrefs)) for name,values in tables.items()};save(output/"table-bounds.json",bounds)
        metadata["zkml-lookup"]=dict(source="zkml",component="official-generate_map-body-compiled",source_body_hash=body_hash,
            relation="unique-entry-lookup; local checker; Halo2 not executed",fractional_bits=16,stored_constant_slots=65537,
            contract="our exhaustive table validation + input quantization/Lipschitz")
        metadata["nfgen-emulator"]=dict(source="nfgen",component="official-SampledPolyFit-and-piece_prediction-executed",
            fractional_bits_parameter=16,representation="decimal emulator; NOT binary Q16.16 MPC execution",
            stored_constant_slots=sum(len(c) for c in chosen["coefficients"])*2+len(chosen["breaks"]),
            contract="our frozen emulator table only; no claim for arbitrary MPC witnesses")
        for name,values in tables.items():
            bound=Q(bounds[name])
            for x in xs:
                y=Q(values[rne(x)])
                rows.append(make_row(name,x,y,refs[str(x)],enclosure=(max(Q(0),y-bound),min(Q(1),y+bound))))
        contracts=bridge.call("contracts");save(output/"contracts.json",contracts)
        rust_validation=[]
        with (output/"our-witnesses.jsonl").open("x") as fw:
            for c in contracts:
                bits=c["format"]["fractional_bits"];name=f"ours-f{bits}"
                print("Our compositional relation:",bits,flush=True);t=time.perf_counter()
                metadata[name]=dict(source="our-extension",component="implicit-root-and-rational-checker",
                    fractional_bits=bits,order=8,stored_constant_slots=16,contract_hash=hash_json(c))
                batch=[];enclosures=[]
                for x in xs:
                    w=own_witness(c,x);fw.write(json.dumps(dict(bits=bits,witness=w),sort_keys=True)+"\n")
                    enclosure=own_check(c,w,x)
                    batch.append(w);enclosures.append(list(map(str,enclosure)))
                    rows.append(make_row(name,x,Q(w["output"]),refs[str(x)],enclosure=enclosure,
                                         relation_evaluations=w["relation_evaluations"]))
                checked=bridge.call("check",contract=c,witnesses=batch,inputs=list(map(str,xs)))
                require(checked==enclosures,"Python/Rust certificate mismatch")
                rust_validation.append(dict(profile=name,checked=len(batch),enclosure_hash=hash_json(checked)))
                timings[name]=time.perf_counter()-t
        save(output/"rust-validation.json",rust_validation)
        save(output/"profiles.json",metadata)
        with (output/"raw.jsonl").open("x") as f:
            for row in rows: f.write(json.dumps(row,sort_keys=True)+"\n")
        with (output/"raw.csv").open("x",newline="") as f:
            writer=csv.DictWriter(f,fieldnames=sorted({k for row in rows for k in row}))
            writer.writeheader();writer.writerows(rows)
        decision_rows=decisions(data,rows,refs,m["thresholds"],bridge)
        save(output/"decisions.json",decision_rows);save(output/"policies.json",policies(decision_rows))
        search_rows=searches(profiles,xs,refs,m["search_ulps"])
        for p in profiles:
            ws=[r["witness"] for r in search_rows if r["profile"]=="sridhar-"+p["method"]]
            result=noir_check(p,ws);require(result["accepted"],"search witnesses failed native Noir")
            differential.append(dict(method=p["method"],case="admissible-search-witnesses",count=len(ws),result=result))
        save(output/"search.json",search_rows);save(output/"native-validation.json",differential)
        timings["total_seconds"]=time.perf_counter()-started;save(output/"timings.json",timings)
        provenance=dict(commit=subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),
            git_status=subprocess.check_output(["git","status","--porcelain"],text=True),
            git_diff_sha256=hashlib.sha256(subprocess.check_output(["git","diff","HEAD"])).hexdigest(),
            platform=platform.platform(),python=sys.version,
            rust=subprocess.check_output(["rustc","--version"],text=True).strip(),
            dependencies=subprocess.check_output([sys.executable,"-m","pip","freeze"],text=True).splitlines(),
            cargo_lock_sha256=digest(ROOT/"Cargo.lock"),
            harness_sources={str(p.relative_to(ROOT)):digest(p) for p in sorted((ROOT/"scripts").glob("literature*.py"))},
            bridge_source_sha256=digest(ROOT/"crates/experiment-runner/src/bin/literature_bridge.rs"))
        provenance["noir_version"]=subprocess.check_output([str(ROOT/"target/literature-artifacts/noir-beta21/nargo"),"--version"],text=True)
        provenance["gnuplot_version"]=subprocess.check_output(["gnuplot","--version"],text=True).strip()
        provenance["contract_kernel_sha256"]=digest(ROOT/"crates/numeric-core/src/contract_certificate.rs")
        save(output/"provenance.json",provenance)
        save(output/"manifest.json",dict(schema_version=1,status="complete-off-circuit",proof_count=0,
            reference="directed-MPFR-256-confirmed-512; table exhaustive at 256",
            files={p.name:digest(p) for p in sorted(output.iterdir()) if p.is_file()}))
    except Exception as exc:
        save(output/"failure.json",dict(status="failed",error=str(exc)))
        raise
    finally: bridge.close()


def verify(run):
    load=lambda name:json.loads((run/name).read_text())
    manifest=load("manifest.json")
    require(manifest["status"]=="complete-off-circuit" and manifest["proof_count"]==0,"incomplete run")
    for name,h in manifest["files"].items(): require(digest(run/name)==h,"digest mismatch: "+name)
    m,data,refs=load("matrix.json"),load("inputs.json"),load("references.json")
    require(m==json.loads((ROOT/"experiments/literature_comparison_v1.json").read_text()),"unauthorized protocol")
    require(load("sources.json")==prepare_sources(),"source lock mismatch")
    require(data==make_inputs(m),"input regeneration mismatch")
    xs=list(map(Q,data["scalars"]));profiles=load("sridhar.json")
    native={"sridhar-"+p["method"]:p for p in profiles}
    tables,bounds=load("tables.json"),load("table-bounds.json")
    gridrefs=load("exhaustive-reference.json")
    for name,values in tables.items(): require(Q(bounds[name])==table_bound(values,gridrefs),"invalid exhaustive bound")
    contracts={c["format"]["fractional_bits"]:c for c in load("contracts.json")}
    bridge=Bridge()
    try:
        require(bridge.call("contracts")==load("contracts.json"),"unauthorized contract constants")
        require(dict(zip(data["scalars"],bridge.refs(xs,512)))==refs,"reference recomputation failed")
        require(bridge.refs([Q(i,65536) for i in range(65537)],256)==gridrefs,"exhaustive reference altered")
    finally: bridge.close()
    for p in profiles:
        regenerated=native_sridhar(p["method"],xs)
        require(all(p[k]==regenerated[k] for k in ("constants","constants_source","witnesses")),"upstream witness/constants differ")
    z,_=native_zkml()
    require(tables["zkml-lookup"]==[str(Q(v,65536)) for v in z],"official lookup mismatch")
    candidates,chosen,predicted=native_nfgen(m["nfgen_degrees"])
    require(predicted==tables["nfgen-emulator"] and chosen==load("nfgen-candidates.json")["selected"],"NFGen regeneration mismatch")
    own={}
    for line in (run/"our-witnesses.jsonl").open():
        r=json.loads(line);w=r["witness"];x=Q(w["input"])
        key=(f"ours-f{r['bits']}",str(x));require(key not in own,"duplicate witness")
        own[key]=(w,own_check(contracts[r["bits"]],w,x))
    bridge=Bridge()
    try:
        for bits,c in contracts.items():
            values=[own[(f"ours-f{bits}",str(x))] for x in xs]
            checked=bridge.call("check",contract=c,witnesses=[w for w,_ in values],inputs=list(map(str,xs)))
            require(checked==[list(map(str,e)) for _,e in values],"independent Rust checker disagrees")
    finally: bridge.close()
    raw=[json.loads(line) for line in (run/"raw.jsonl").open()]
    require(len(raw)==len(xs)*10,"missing raw observations")
    seen=set(); positions={str(x):i for i,x in enumerate(xs)}
    for r in raw:
        name,x,y=r["profile"],Q(r["input"]),Q(r["output"]);key=(name,str(x))
        require(key not in seen,"duplicate observation"); seen.add(key)
        require(error_bounds(y,refs[str(x)])==tuple(Q(r[k]) for k in ("error_lower","error_upper")),"wrong error")
        if name in native:
            p=native[name];w=p["witnesses"][positions[str(x)]]
            v=w["inp_struct"] if p["method"]=="gl" else w
            require(int(v["x"])==rne(x) and y==Q(int(v["y"]),65536),"native input/output mismatch")
            require(r["accepted"]==accept_sridhar(p,w) and r["lower"] is None and r["upper"] is None,"native relation/guarantee mismatch")
        else:
            if name in tables:
                require(y==Q(tables[name][rne(x)]),"table output mismatch")
                b=Q(bounds[name]);enclosure=(max(Q(0),y-b),min(Q(1),y+b))
            else:
                w,enclosure=own[key];require(y==Q(w["output"]),"our output mismatch")
            require(r["accepted"] and enclosure==(Q(r["lower"]),Q(r["upper"])),"invalid enclosure")
    bridge=Bridge()
    try: actual=decisions(data,raw,refs,m["thresholds"],bridge)
    finally: bridge.close()
    require(actual==load("decisions.json"),"decision audit mismatch")
    require(policies(actual)==load("policies.json"),"adaptive replay mismatch")
    for r in load("search.json"):
        p=native[r["profile"]]; require(accept_sridhar(p,r["witness"]),"invalid adversarial witness")
        v=r["witness"]["inp_struct"] if p["method"]=="gl" else r["witness"]
        require(int(v["x"])==rne(Q(r["input"])),"search switched input")
        require(error_bounds(Q(int(v["y"]),65536),refs[r["input"]])[1]==Q(r["largest_found_error"]),"wrong search error")
    print(json.dumps(dict(status="verified-off-circuit",scalar_rows=len(raw),decisions=len(actual),proof_count=0)))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command",choices=["prepare","generate","verify"])
    parser.add_argument("--run-dir",type=Path)
    parser.add_argument("--matrix",type=Path,default=ROOT/"experiments/literature_comparison_v1.json")
    args=parser.parse_args()
    if args.command=="prepare": prepare_sources()
    elif args.command=="generate": generate(args.run_dir,args.matrix)
    else: verify(args.run_dir)


if __name__=="__main__": main()

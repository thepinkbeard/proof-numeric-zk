#!/usr/bin/env python3
"""Paired percentile bootstrap on round means; one scenario is not a population."""
import argparse
import itertools
import json
import random
import statistics
from collections import defaultdict
from pathlib import Path

def main():
    p=argparse.ArgumentParser();p.add_argument("--input-dir",type=Path,required=True);p.add_argument("--output",type=Path,required=True);a=p.parse_args()
    data=json.loads((a.input_dir/"timing.json").read_text());groups=defaultdict(dict)
    for r in data:
        if not r["warmup"]:groups[(r["scenario"],r["policy"],r["fixed_precision"])][r["round"]]=r
    comparisons=[]
    for ka,kb in itertools.combinations(sorted(groups),2):
        if ka[0]!=kb[0]:continue
        ga,gb=groups[ka],groups[kb];rounds=sorted(set(ga)&set(gb))
        if not rounds:continue
        equal_coverage=all(ga[i]["certified_family"] and gb[i]["certified_family"] and (ga[i]["decision"]!="Indeterminate")== (gb[i]["decision"]!="Indeterminate") for i in rounds)
        positive_coverage=any(ga[i]["decision"]!="Indeterminate" for i in rounds)
        for metric in ["complete_wall_seconds","setup_excluded_seconds"]:
            av=[ga[i][metric] for i in rounds];bv=[gb[i][metric] for i in rounds]
            rng=random.Random(20260911);boot=[]
            if len(rounds)>=30:
                for _ in range(10000):
                    sample=[rng.randrange(len(rounds)) for _ in rounds]
                    boot.append(1-statistics.mean(av[i] for i in sample)/statistics.mean(bv[i] for i in sample))
                boot.sort()
            lower=boot[250] if boot else None;upper=boot[9749] if boot else None
            reduction=1-statistics.mean(av)/statistics.mean(bv)
            comparisons.append({"scenario":ka[0],"a":ka[1:],"b":kb[1:],"rounds":len(rounds),"metric":metric,
                "mean_reduction_a_over_b":reduction,"ci95":[lower,upper],"equal_certified_coverage":equal_coverage,
                "positive_certified_coverage":equal_coverage and positive_coverage,
                "pairwise_10_percent_reduction":bool(equal_coverage and positive_coverage and lower is not None and lower>0 and reduction>=.1),
                "scope":"fixed public scenario; no population/general speedup claim","multiplicity_adjusted":False})
    with a.output.open("x") as f:json.dump(comparisons,f,indent=2)
if __name__=="__main__":main()

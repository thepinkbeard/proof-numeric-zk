#!/usr/bin/env python3
"""Aggregate immutable qualification data and generate reproducible Gnuplot figures."""
import argparse
import csv
import json
import math
import statistics
import subprocess
from collections import defaultdict
from pathlib import Path

def quoted(s):
    return '"' + str(s).replace('\\', '\\\\').replace('"','\\"').replace('\n',' ') + '"'

def write_data(path, rows):
    with path.open("x") as f:
        for row in rows:
            f.write(" ".join(map(str,row))+"\n")

def figure(out, number, title, xlabel, ylabel, groups, logy=False, secondary=None):
    specs=[]
    named_specs=[]
    for i,(name,points) in enumerate(groups.items()):
        if not points: continue
        path=out/f"{number:02d}-{i}.dat"
        write_data(path, sorted(points))
        specs.append(f'{quoted(path.name)} using 1:2 with linespoints lw 1.7 pt {i%8+1} title {quoted(name)}')
        named_specs.append((name,specs[-1]))
    if secondary:
        for i,(name,points) in enumerate(secondary.items()):
            path=out/f"{number:02d}-secondary-{i}.dat";write_data(path,sorted(points))
            specs.append(f'{quoted(path.name)} using 1:2 axes x1y2 with linespoints dt 2 lw 1.7 title {quoted(name)}')
    base=["set encoding utf8", "set border 3", "set tics nomirror", "set grid back lc rgb '#e2e8f0'",
          "set key outside bottom center horizontal maxrows 4 font ',10'",
          "set lmargin 11", "set rmargin 3", "set bmargin 8", "set tmargin 4",
          f"set title {quoted(title)}", f"set xlabel {quoted(xlabel)}",f"set ylabel {quoted(ylabel)}"]
    if logy:base.append("set logscale y")
    if secondary:base.extend(["set y2tics","set y2label 'mean selected fractional bits'","set rmargin 11"])
    if not specs:
        base.extend(["set xrange [0:1]","set yrange [0:1]","unset key",
            "set label 'NOT MEASURED - no cost claim' at graph 0.5,0.5 center"])
        plot="plot NaN notitle"
    else: plot="plot "+", \\\n+     ".join(specs)
    script="\n".join(base)+"\n"
    for terminal,ext in [("pdfcairo enhanced color size 10in,6in font 'Arial,11'","pdf"),("svg size 1100,660 enhanced font 'Arial,13'","svg")]:
        script+=f"set terminal {terminal}\nset output '{number:02d}.{ext}'\n"
        if number==1:
            script+="set bmargin 4\nset tmargin 3\nset key font ',9' maxrows 1\nset multiplot layout 2,1\n"
            for prefix,regime in [("7/2","r=3.5 - stable regime"),("39/10","r=3.9 - chaotic regime")]:
                panel=[s for name,s in named_specs if name.startswith(prefix+" ")]
                script+=f"set title {quoted(regime+'; x0=1/10')}\nplot "+", ".join(panel)+"\n"
            script+="unset multiplot\n"
        else:script+=plot+"\n"
        script+="unset output\n"
    path=out/f"{number:02d}.gp"
    path.write_text(script)
    subprocess.run(["gnuplot",path.name],cwd=out,check=True)

def main():
    p=argparse.ArgumentParser();p.add_argument("--input-dir",type=Path,required=True);p.add_argument("--output-dir",type=Path,required=True);p.add_argument("--timing-dir",type=Path)
    a=p.parse_args();manifest=json.loads((a.input_dir/"manifest.json").read_text())
    if manifest["status"]!="numeric-complete":raise ValueError("incomplete numeric run")
    a.output_dir.mkdir()
    aggregates=defaultdict(lambda:{"n":0,"flips":0,"resolved":0,"stable":0,"abstain":0,"bits":0,"evaluations":0,"first":[],"unknown":0})
    numeric_count=0;certified_errors=0
    with (a.input_dir/"raw.jsonl").open() as f:
        for line in f:
            r=json.loads(line);numeric_count+=1
            key=tuple(r[k] for k in ["input_set","workload","parameter","family","precision","horizon","threshold"])
            s=aggregates[key];s["n"]+=1;s["resolved"]+=r["reference_decision"] is not None;s["unknown"]+=r["reference_decision"] is None
            s["flips"]+=r["predicate_flip"] is True;s["stable"]+=r["certified"] and r["decision"]!="Indeterminate"
            s["abstain"]+=r["decision"]=="Indeterminate";s["bits"]+=r["precision"];s["evaluations"]+=len(r["evaluations"])
            if r["first_abstention"] is not None:s["first"].append(r["first_abstention"])
            certified_errors+=r["certified"] and r["predicate_flip"] is True
    if certified_errors:raise ValueError("certified contradiction invalidates figures")
    fields=["input_set","workload","parameter","family","precision","horizon","threshold","n","resolved","unknown","flips","stable","abstain","coverage","flip_rate","mean_evaluations","mean_first_abstention"]
    summary=[]
    for key,s in sorted(aggregates.items()):
        row=dict(zip(fields[:7],key));row.update({k:s[k] for k in fields[7:13]})
        row.update(coverage=s["stable"]/s["n"],flip_rate=s["flips"]/s["resolved"] if s["resolved"] else None,mean_evaluations=s["evaluations"]/s["n"],mean_first_abstention=statistics.mean(s["first"]) if s["first"] else None)
        summary.append(row)
    with (a.output_dir/"summary.csv").open("x",newline="") as f:
        w=csv.DictWriter(f,fields);w.writeheader();w.writerows(summary)
    # Raw CSV export is streaming; JSON arrays are retained as JSON cells.
    with (a.input_dir/"raw.jsonl").open() as src,(a.output_dir/"raw.csv").open("x",newline="") as dst:
        writer=None
        for line in src:
            row=json.loads(line)
            if writer is None:writer=csv.DictWriter(dst,row.keys());writer.writeheader()
            writer.writerow({k:json.dumps(v) if isinstance(v,(list,dict)) else v for k,v in row.items()})
    with (a.input_dir/"trajectories.csv").open() as f:traj=list(csv.DictReader(f))
    groups=defaultdict(list);errors=defaultdict(list)
    for r in traj:
        if r["workload"]!="logistic" or r["parameter"] not in ("7/2","39/10"):continue
        if r["family"] in ("fixed-rne","fp32") and r["precision"] in ("16","24") and int(r["iteration"])<=100:
            name='FP32' if r["family"]=='fp32' else f'{r["family"]} F{r["precision"]}'
            groups[f'{r["parameter"]} {name}'].append((int(r["iteration"]),float(r["center"])))
        if r["family"]=="fixed-rne" and r["precision"]=="16" and int(r["iteration"])<=100:
            groups[f'{r["parameter"]} reference'].append((int(r["iteration"]),float(r["reference_center"])))
        if r["parameter"]=="39/10" and r["family"] in ("fixed-rne","cbd-local") and r["precision"] in ("16","64"):
            errors[f'{r["family"]} F{r["precision"]} center error'].append((int(r["iteration"]),max(float(r["error"]),1e-20)))
            if r["family"]=="cbd-local":errors[f'CBD F{r["precision"]} width'].append((int(r["iteration"]),max(float(r["width"]),1e-20)))
    figure(a.output_dir,1,"Historical trajectory x0=1/10: two regimes","iteration","state",groups)
    figure(a.output_dir,2,"r=3.9, historical input; display floor 1e-20","iteration","error / enclosure width",errors,True)
    sample="sample" if any(r["input_set"]=="sample" for r in summary) else "historical"
    chosen=[r for r in summary if r["input_set"]==sample and r["workload"]=="logistic" and r["parameter"]=="39/10" and r["threshold"]=="1/2"]
    rates=defaultdict(list);coverage=defaultdict(list);policies=defaultdict(list)
    for r in chosen:
        if (r["family"] in ("fixed-rne","cbd-local") and r["precision"] in (16,64)) or r["family"]=="fp32":
            name='FP32' if r["family"]=='fp32' else f'{r["family"]} F{r["precision"]}'
            if r["family"]=="cbd-local":rates[name+" abstention"].append((r["horizon"],r["abstain"]/r["n"]))
            elif r["flip_rate"] is not None:rates[name+" flip"].append((r["horizon"],r["flip_rate"]))
        if r["family"]=="cbd-local":coverage[f'H={r["horizon"]}'].append((r["precision"],r["coverage"]))
    # Recombine adaptive rows split by selected precision before computing means.
    adaptive=defaultdict(lambda:[0,0,0])
    for r in chosen:
        if r["family"].startswith("cdad-"):
            s=adaptive[(r["family"],r["horizon"])];s[0]+=r["n"];s[1]+=r["precision"]*r["n"];s[2]+=r["mean_evaluations"]*r["n"]
    precisions=defaultdict(list)
    for (family,h),(n,bits,ev) in adaptive.items():
        policies[family.removeprefix("cdad-")+" evaluations"].append((h,ev/n))
        precisions[family.removeprefix("cdad-")+" precision"].append((h,bits/n))
    figure(a.output_dir,3,f"r=3.9, tau=0.5; {sample} inputs","horizon","flip / abstention fraction",rates)
    figure(a.output_dir,4,f"CBD coverage: r=3.9, tau=0.5; {sample}","fractional bits","certified fraction",coverage)
    figure(a.output_dir,5,f"Adaptive evaluations from x0; {sample}","horizon","mean evaluations",policies,secondary=precisions)
    costs=defaultdict(list);pareto=defaultdict(list);timing_summary=[];cost_scope="not measured"
    if a.timing_dir:
        timing_manifest=json.loads((a.timing_dir/"manifest.json").read_text())
        if timing_manifest["status"]!="proof-campaign-complete":raise ValueError("incomplete proof campaign")
        records=json.loads((a.timing_dir/"timing.json").read_text())
        buckets=defaultdict(list)
        for r in records:
            if not r["warmup"] and r["scenario"]=="boundary-10":buckets[(r["scenario"],r["policy"],r["fixed_precision"])].append(r)
        for (scenario,policy,bits),rs in buckets.items():
            cost=statistics.mean(r["complete_wall_seconds"] for r in rs)
            cov=sum(r["decision"]!="Indeterminate" and not r.get("predicate_flip",False) for r in rs)/len(rs)
            label=policy.removeprefix("baseline-")+(f' F{bits}' if bits!=24 else ' p24')
            costs[label].append((statistics.mean(r["primary_constraints"] for r in rs),cost))
            if all(r.get("certified_family",True) for r in rs):pareto[label].append((cost,cov))
            timing_summary.append({"label":label,"mean_total":cost,
                "mean_no_setup":statistics.mean(r["setup_excluded_seconds"] for r in rs),
                "mean_primary_constraints":statistics.mean(r["primary_constraints"] for r in rs),
                "certified_decisions":sum(r["decision"]!="Indeterminate" for r in rs),
                "flips":sum(bool(r.get("predicate_flip",False)) for r in rs)})
        cost_scope=f"boundary-10, r=4; {timing_manifest['rounds']} rounds"
    figure(a.output_dir,6,f"Actual proof costs: {cost_scope}","sum augmented constraints per step across attempts","seconds (includes setup)",costs)
    figure(a.output_dir,7,f"Cost and certified availability: {cost_scope}","full seconds (includes setup)","definitive fraction",pareto)
    text=f"""# Relatório gerado da qualificação

Estado numérico: {manifest['status']}. Matriz completa: {manifest['full_matrix']}.
Registros: {numeric_count}. Decisões certificadas contraditórias: {certified_errors}.
Resultados agregados estão em summary.csv; dados por oportunidade em raw.csv.

## Leitura das figuras

1. Trajetórias históricas predefinidas. Divergência não implica falha criptográfica.
2. Erro central e largura do contrato. Piso 1e-20 é apenas de visualização.
3. Flips dos baselines e abstenções CBD são eventos distintos, não taxas equivalentes.
4. Cobertura por precisão e horizonte. Aumentar precisão não elimina o limite caótico.
5. Avaliações locais necessárias; não são provas executadas na matriz numérica.
6. Custo de provas curtas quando fornecido. Setup está incluído e precisa ser separado para inferência sobre custo recorrente.
7. Disponibilidade dos cenários curtos; não é fronteira populacional nem comparação com papers externos.

As figuras 3-5 usam o grupo {sample}, sem mistura com adversariais. A tabela
preserva os grupos, parâmetros e limiares separados. Referências inconclusivas
não entram no denominador de flips. Sucesso de testes não substitui demonstração
matemática de inclusão nem prova de correspondência com a implementação.

O benchmark CFD usa corredor limitado e o FP32 é restrito. Ambos são baselines
locais, não reproduções integrais de artefatos externos. Custos ausentes não são
estimados a partir da literatura. Consulte docs/qualification.md para protocolo,
fontes primárias e limites da entrega.
"""
    text+="\n## Comparação numérica predefinida: r=3,9; limiar=0,5\n\n"
    text+="Grupo: "+sample+". Cada oportunidade é a decisão no horizonte final, não cada passo.\n\n"
    text+="| Família | Precisão | Horizonte | Casos | Flips | Definitivas certificadas | Abstenções |\n|---|---:|---:|---:|---:|---:|---:|\n"
    for r in chosen:
        if r["horizon"] in (10,1000) and ((r["family"] in ("cbd-local","fixed-rne") and r["precision"] in (16,64)) or r["family"]=="fp32"):
            text+=f'| {r["family"]} | {r["precision"]} | {r["horizon"]} | {r["n"]} | {r["flips"]} | {r["stable"]} | {r["abstain"]} |\n'
    text+="\nPrecisão FP32=24 significa bits do significando; não 24 bits fracionários. Zero definitivas certificadas nos baselines sem bound não significa zero acertos observados.\n"
    if timing_summary:
        text+="\n## Custos observados da campanha\n\n"
        text+="Cenário: boundary-10, com 30 rodadas sem aquecimento. Médias por configuração; setup é mostrado separadamente e não foi reutilizado.\n\n"
        text+="| Configuração | Tempo total (s) | Sem setup (s) | Constraints aumentadas | Decisões certificadas | Flips observados |\n|---|---:|---:|---:|---:|---:|\n"
        for r in sorted(timing_summary,key=lambda x:x["label"]):
            text+=f'| {r["label"]} | {r["mean_total"]:.4f} | {r["mean_no_setup"]:.4f} | {r["mean_primary_constraints"]:.0f} | {r["certified_decisions"]} | {r["flips"]} |\n'
        text+="\nEsses tempos são deste cenário público e deste host; não sustentam uma afirmação de ganho populacional.\n"
    (a.output_dir/"report.md").write_text(text)
    (a.output_dir/"manifest.json").write_text(json.dumps({"status":"figures-complete","gnuplot":subprocess.check_output(["gnuplot","--version"],text=True).strip(),"input_manifest":manifest,"cost_data_provided":bool(a.timing_dir)},indent=2))
if __name__=="__main__":main()

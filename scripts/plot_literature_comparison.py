#!/usr/bin/env python3
"""Six source-labelled figures. Every plotted number comes from a verified run.

Raw fractions are retained in the experiment; plotting conversion alone is f64.
No proof time or backend-independent constraint proxy is invented.
"""
import argparse
from collections import Counter, defaultdict
import csv
from fractions import Fraction as Q
import json
from pathlib import Path

from literature_artifacts import digest, require, save
from qualification_group_a import data_file, emit_figure, panel_layout, quote

ORDER=["sridhar-gl","sridhar-poly","sridhar-pade","sridhar-pwl","zkml-lookup","nfgen-emulator","ours-f16"]
LABELS={"sridhar-gl":"Sridhar26 GL", "sridhar-poly":"Sridhar26 Poly", "sridhar-pade":"Sridhar26 Pade",
        "sridhar-pwl":"Sridhar26 PWL", "zkml-lookup":"ZKML24 lookup", "nfgen-emulator":"NFGen22 emul.",
        "ours-f16":"Nosso GL8 F16", "ours-f32":"Nosso GL8 F32", "ours-f48":"Nosso GL8 F48", "ours-f64":"Nosso GL8 F64"}
COLORS=["#3d638f","#855aa5","#b0833b","#507f92","#c25d44","#8d823e","#178367"]
STATES=["certified","observed_same","flip","abstention","rejection","unresolved"]
STATE_LABELS=["Certificada","Concordancia observada","Divergencia","Abstencao","Witness rejeitado","Ref. inconclusiva"]
STATE_COLORS=["#178367","#7299bd","#d45943","#e1b153","#754b70","#aaaaaa"]


def outcome(row,plain=False):
    if not row["accepted"]: return "rejection"
    if row["reference_decision"] is None: return "unresolved"
    if not plain and row["contract_available"]:
        return "certified" if row["certified_decision"] is not None else "abstention"
    return "flip" if row["observed_flip"] else "observed_same"


def aggregate(rows,plain=False):
    counts=Counter(outcome(r,plain) for r in rows)
    return [100*counts[k]/len(rows) for k in STATES]


def bars(out,filename,labels,values,titles,colors,ylim="[0:100]"):
    f=data_file(out,filename,[[i,*v] for i,v in enumerate(values)])
    ticks=", ".join(f"{quote(label)} {i}" for i,label in enumerate(labels))
    text=f"unset logscale\nset xtics border\nset ytics border autofreq\nset xrange [-0.6:{len(labels)-.4}]\nset yrange {ylim}\nset xtics ({ticks}) rotate by -25 font ',9'\nset boxwidth 0.7\n"
    # Manual stacking avoids gnuplot's clustered/stacked data interpretation.
    plots=[]
    for j,(title,color) in enumerate(zip(titles,colors)):
        bottom="+".join(f"${k+2}" for k in range(j)) or "0"
        top="+".join(f"${k+2}" for k in range(j+1))
        plots.append(f"{f} using 1:(({bottom}+{top})/2):($1-0.34):($1+0.34):({bottom}):({top}) with boxxyerror fc rgb '{color}' title {quote(title)}")
    return text+"plot "+", ".join(plots)+"\n"


def render(run,out,pdf):
    require(not out.exists() and not pdf.exists(),"refusing to overwrite figures/PDF")
    manifest=json.loads((run/"manifest.json").read_text())
    require(manifest["status"]=="complete-off-circuit","incomplete numerical campaign")
    for name,sha in manifest["files"].items(): require(digest(run/name)==sha,"modified input: "+name)
    out.mkdir(parents=True);pdf.parent.mkdir(parents=True,exist_ok=True)
    load=lambda n:json.loads((run/n).read_text())
    rows=[json.loads(line) for line in (run/"raw.jsonl").open()]
    data,decisions,policies=load("inputs.json"),load("decisions.json"),load("policies.json")
    metadata,searches=load("profiles.json"),load("search.json")
    groups=defaultdict(list)
    for row in rows: groups[row["profile"]].append(row)
    summary={"run_manifest_sha256":digest(run/"manifest.json"),"proof_count":0,
             "independent_random_vectors":sum(s["cohort"]=="random" for s in data["samples"]),
             "adversarial_vectors":sum(s["cohort"]=="adversarial" for s in data["samples"]),"profiles":{},"policies":{}}
    for name,values in groups.items():
        info={"max_absolute_error_upper":max(float(Q(r["error_upper"])) for r in values),
              "scalar_witness_rejections":sum(not r["accepted"] for r in values),"cohorts":{}}
        for cohort in ("random","adversarial"):
            ds=[r for r in decisions if r["profile"]==name and r["cohort"]==cohort]
            info["cohorts"][cohort]={"opportunities":len(ds),**dict(Counter(outcome(r) for r in ds)),
                                    "honest_observed_flips":sum(r["observed_flip"] is True for r in ds)}
        summary["profiles"][name]=info
    for policy in sorted(set(r["policy"] for r in policies)):
        summary["policies"][policy]={}
        for cohort in ("random","adversarial"):
            ps=[r for r in policies if r["policy"]==policy and r["cohort"]==cohort]
            summary["policies"][policy][cohort]=dict(opportunities=len(ps),
                certified=sum(r["certified_decision"] is not None for r in ps),
                mean_evaluations=sum(len(r["evaluated_bits"]) for r in ps)/len(ps),
                selected_precision=dict(Counter(r["selected_bits"] for r in ps)))
    save(out/"summary.json",summary)

    body=panel_layout("1 | Saidas honestas dos componentes publicados",1,2,[
        "Pergunta: qual o erro das saidas honestas nas mesmas entradas? Referencia MPFR dirigida; dominio [0,1].",
        "Os quatro geradores de Sridhar quantizam a mesma sigmoid de alta precisao: suas curvas coincidem.",
        "F16 comum para entrada; NFGen usa seu emulador decimal. Sao componentes reproduzidos, nao provas completas."],
        margins="0.10,0.96,0.20,0.90",spacing="0.14,0.12")
    body+="set title 'Malha de 1.025 pontos'\nset xlabel 'Entrada x'\nset ylabel 'Erro absoluto (limite superior MPFR)'\nset format y '%.0e'\nset logscale y\nset yrange [1e-8:1e-3]\nset xrange [0:1]\nset key top left font ',8'\n"
    plots=[];grid=set(data["grid"])
    for i,name in enumerate(["sridhar-gl","zkml-lookup","nfgen-emulator","ours-f16"]):
        values=sorted((r for r in groups[name] if r["input"] in grid),key=lambda r:Q(r["input"]))
        f=data_file(out,"01-"+name+".dat",[[float(Q(r["input"])),float(Q(r["error_upper"]))] for r in values])
        title="Sridhar26: GL/Poly/Pade/PWL" if name=="sridhar-gl" else LABELS[name]
        plots.append(f"{f} using 1:($2>0?$2:1/0) with lines lw 1 lc rgb '{COLORS[[0,4,5,6][i]]}' title {quote(title)}")
    body+="plot "+", ".join(plots)+"\n"
    body+="set title 'Maior erro no conjunto escalar'\nunset key\nset xlabel ''\nset format y '%.1e'\nset ylabel 'Maximo observado (nao bound global)'\nunset logscale\n"
    body+=bars(out,"01-max.dat",[LABELS[n] for n in ORDER],[[summary["profiles"][n]["max_absolute_error_upper"]] for n in ORDER],["Erro"],["#507f92"],"[0:*]")
    emit_figure(out,1,body,height=9)

    body=panel_layout("2 | Saida honesta versus liberdade aceita pela relacao",1,2,[
        "Pergunta: o erro honesto descreve toda a liberdade do witness? Busca em 5 entradas, +/-256 ULP na saida.",
        "Os 20 witnesses extremos selecionados foram aceitos pelo Noir original. Busca limitada nao e maximo universal.",
        "Painel direito: contratos nossos, de escopos diferentes; nao e uma comparacao de soundness dos sistemas ZK."])
    native=ORDER[:4];points=[]
    for i,name in enumerate(native):
        ss=[r for r in searches if r["profile"]==name]
        points.append([i,max(float(Q(r["honest_error"])) for r in ss),max(float(Q(r["largest_found_error"])) for r in ss)])
    f=data_file(out,"02-admissible.dat",points)
    ticks=", ".join(f"{quote(LABELS[n])} {i}" for i,n in enumerate(native))
    body+=f"set title 'Relacoes nativas, F16'\nset xlabel ''\nset ylabel 'Desvio absoluto'\nset format y '%.0e'\nset xtics ({ticks}) rotate by -25\nset xrange [-0.5:3.5]\nset logscale y\nset yrange [1e-7:1e-2]\nset key top left\nplot {f} using 1:2 with points pt 7 ps 1.3 title 'Honesto', {f} using 1:3 with points pt 5 ps 1.3 title 'Maior encontrado'\n"
    names=ORDER[4:];bound_rows=[]
    for n in names:
        bound_rows.append([max(float((Q(r["upper"])-Q(r["lower"]))/2) for r in groups[n])])
    body+="set title 'Raio maximo dos contratos adicionados'\nset ylabel 'Raio certificado no escopo declarado'\nset format y '%.1e'\nunset key\n"
    body+=bars(out,"02-bounds.dat",[LABELS[n] for n in names],bound_rows,["Raio"],["#178367"],"[0:*]")
    emit_figure(out,2,body,height=9)

    body=panel_layout("3 | Media de quatro sigmoids: o que sustenta a decisao?",1,2,[
        "Pergunta: a composicao preserva a decisao? Limiares 0,55 / 0,625 / 0,70; mesmos vetores para todos.",
        "Aleatorio: 1.000 vetores em 10 seeds (3.000 predicados correlacionados). Adversariais separados das taxas.",
        "Azul nao implica ausencia de garantia no paper; verde usa nossos contratos (NFGen: emulador, nao MPC)."])
    for i,cohort in enumerate(("random","adversarial")):
        vals=[aggregate([r for r in decisions if r["profile"]==n and r["cohort"]==cohort]) for n in ORDER]
        body+=f"set title {quote('Amostra aleatoria' if i==0 else 'Fronteiras e extremos')}\nset xlabel ''\nset ylabel 'Oportunidades (%)'\n"
        body+=("set key at screen 0.5,0.955 center horizontal maxrows 2 font ',9'\n" if i==0 else "unset key\n")
        body+=bars(out,f"03-{cohort}.dat",[LABELS[n] for n in ORDER],vals,STATE_LABELS,STATE_COLORS)
    emit_figure(out,3,body,height=9)

    body=panel_layout("4 | Isolar o ganho do contrato: mesmos valores computados",1,2,[
        "Pergunta: o contrato evita emitir uma decisao nao sustentada? Para cada metodo: sem contrato / com contrato.",
        "Nenhuma saida foi recalculada para favorecer o contrato. Abster-se nao melhora o erro da aproximacao.",
        "A garantia e da relacao numerica declarada; nenhuma prova ZK ou execucao MPC foi produzida nesta campanha."])
    for i,cohort in enumerate(("random","adversarial")):
        labels=[];vals=[]
        for name in ORDER[4:]:
            rs=[r for r in decisions if r["profile"]==name and r["cohort"]==cohort]
            for plain in (True,False):
                labels.append(LABELS[name]+(" sem" if plain else " com"));vals.append(aggregate(rs,plain))
        body+=f"set title {quote('Aleatorios' if i==0 else 'Adversariais')}\nset ylabel 'Oportunidades (%)'\nset xlabel ''\n"
        body+=("set key at screen 0.5,0.955 center horizontal maxrows 2 font ',9'\n" if i==0 else "unset key\n")
        body+=bars(out,f"04-{cohort}.dat",labels,vals,STATE_LABELS,STATE_COLORS)
    emit_figure(out,4,body,height=9)

    body=panel_layout("5 | Precisao fixa versus adaptacao: contrato GL8 identico",1,2,[
        "Pergunta: adaptar aumenta cobertura e quantas avaliacoes exige? Escada F16/F32/F48/F64; mesma entrada inicial.",
        "Contagem por replay dos perfis: nao e tempo medido de uma politica em execucao nem numero de provas.",
        "As politicas consultam apenas margem e enclosure. A referencia MPFR nao participa da selecao."])
    pol=["fixed16","fixed64","sequential","margin","preview-all"]
    labels=["F16","F64","Escada","Margem","Preview todos"]
    values=[]
    for i,p in enumerate(pol):
        a=summary["policies"][p]["random"];b=summary["policies"][p]["adversarial"]
        values.append([i,100*a["certified"]/a["opportunities"],100*b["certified"]/b["opportunities"],a["mean_evaluations"],b["mean_evaluations"]])
    f=data_file(out,"05-policies.dat",values)
    ticks=", ".join(f"{quote(n)} {i}" for i,n in enumerate(labels))
    body+=f"set xtics ({ticks}) rotate by -20\nset xrange [-0.5:4.5]\nset yrange [0:105]\nset ylabel 'Cobertura certificada (%)'\nset title 'Cobertura por politica'\nset key bottom left\nplot {f} using 1:2 with linespoints pt 7 title 'Aleatorios', {f} using 1:3 with linespoints pt 5 title 'Adversariais'\n"
    body+=f"set title 'Numero medio de perfis avaliados'\nset yrange [0:4.5]\nset ylabel 'Avaliacoes por oportunidade'\nset key top left\nplot {f} using 1:4 with linespoints pt 7 title 'Aleatorios', {f} using 1:5 with linespoints pt 5 title 'Adversariais'\n"
    emit_figure(out,5,body,height=9)

    body=panel_layout("6 | Recursos numericos versus qualidade: sem custo ZK inventado",1,2,[
        "Pergunta: quais recursos acompanham a precisao e a garantia? Constantes contabilizadas na configuracao usada.",
        "ZKML: 65.537 entradas utilizadas de uma tabela maior. NFGen: coeficientes, escalas e fronteiras; nosso GL: nos/pesos.",
        "Contagem de constantes nao e memoria medida, constraints ou custo de prova. Nao ha ranking de backends."])
    pts=[]
    for i,name in enumerate(ORDER):
        stats=summary["profiles"][name];co=stats["cohorts"]["adversarial"]
        pts.append([metadata[name]["stored_constant_slots"],stats["max_absolute_error_upper"],100*co.get("certified",0)/co["opportunities"]])
    f=data_file(out,"06-resources.dat",pts)
    body+="set title 'Erro honesto e armazenamento de constantes'\nset format y '%.0e'\nset logscale xy\nset xrange [3:300000]\nset yrange [1e-6:1e-3]\nset xlabel 'Valores constantes usados (nao bytes)'\nset ylabel 'Maior erro observado'\nset key top right font ',8'\n"
    body+="plot "+", ".join(f"{f} every ::{i}::{i} using 1:2 with points pt {i+4} ps 1.3 lc rgb '{COLORS[i]}' title {quote(LABELS[n])}" for i,n in enumerate(ORDER))+"\n"
    body+="unset logscale y\nset format y '%g'\nset title 'Cobertura adversarial onde ha contrato validado'\nset yrange [0:105]\nset ylabel 'Cobertura certificada (%)'\nset key bottom left font ',8'\n"
    body+="plot "+", ".join(f"{f} every ::{i}::{i} using 1:3 with points pt {i+4} ps 1.4 lc rgb '{COLORS[i]}' title {quote(LABELS[ORDER[i]])}" for i in [4,5,6])+"\n"
    emit_figure(out,6,body,height=9)
    from pypdf import PdfWriter
    writer=PdfWriter()
    for i in range(1,7): writer.append(out/f"{i:02}.pdf")
    with pdf.open("xb") as f:writer.write(f)
    writer.close()
    with (out/"summary.csv").open("x",newline="") as f:
        fields=["profile","cohort","opportunities",*STATES,"honest_observed_flips"]
        w=csv.DictWriter(f,fieldnames=fields);w.writeheader()
        for name,info in summary["profiles"].items():
            for cohort,count in info["cohorts"].items():
                w.writerow(dict(profile=name,cohort=cohort,**{k:count.get(k,0) for k in fields[2:]}))
    save(out/"manifest.json",dict(source_run_manifest_sha256=digest(run/"manifest.json"),
        files={p.name:digest(p) for p in sorted(out.iterdir()) if p.is_file()},pdf_sha256=digest(pdf)))


if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run-dir",required=True,type=Path);p.add_argument("--output-dir",required=True,type=Path)
    p.add_argument("--pdf",required=True,type=Path);a=p.parse_args();render(a.run_dir,a.output_dir,a.pdf)

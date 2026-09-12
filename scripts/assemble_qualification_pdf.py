#!/usr/bin/env python3
"""Optional presentation convenience: concatenate the seven Gnuplot PDFs.

Requires pypdf; the core experiment and individual figures do not.
"""
import argparse
from pathlib import Path
from pypdf import PdfReader, PdfWriter

def main():
    p=argparse.ArgumentParser();p.add_argument("--figures",type=Path,required=True);p.add_argument("--output",type=Path,required=True);a=p.parse_args()
    writer=PdfWriter()
    for i in range(1,8):
        path=a.figures/f"{i:02d}.pdf"
        reader=PdfReader(path)
        if len(reader.pages)!=1:raise ValueError(f"expected one page in {path}")
        writer.append(reader,outline_item=f"Figure {i}")
    a.output.parent.mkdir(parents=True,exist_ok=True)
    with a.output.open("xb") as f:writer.write(f)
    if len(PdfReader(a.output).pages)!=7:raise ValueError("incomplete report")
if __name__=="__main__":main()

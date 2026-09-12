#!/usr/bin/env python3
"""Unit tests; optional real-proof tampering via QUAL_BUNDLE and QUAL_STATEMENT."""
import copy
import json
import os
import shutil
import tempfile
import unittest
from fractions import Fraction
from pathlib import Path
import qualification as q

class PublicContractTests(unittest.TestCase):
    def test_rne_signed_ties(self):
        for value,expected in [(Fraction(1,2),0),(Fraction(3,2),2),(Fraction(5,2),2),(-Fraction(1,2),0),(-Fraction(3,2),-2)]:
            self.assertEqual(q.rne(value),expected)

    def test_invalid_statements(self):
        valid={"schema_version":1,"x0":"1/10","r":"4","threshold":"1/2","horizon":2,"public_inputs":True}
        q.validate_statement(valid)
        for key,value in [("horizon",0),("x0","2"),("r","5"),("threshold","3/4"),("public_inputs",False)]:
            bad={**valid,key:value}
            with self.assertRaises(ValueError):q.validate_statement(bad)

    def test_scenarios_reproduce(self):
        with tempfile.TemporaryDirectory() as root:
            a=Path(root)/"a";b=Path(root)/"b";q.scenarios(a);q.scenarios(b)
            for p in a.iterdir():self.assertEqual(p.read_bytes(),(b/p.name).read_bytes())
            with self.assertRaises(FileExistsError):q.scenarios(a)

@unittest.skipUnless(os.environ.get("QUAL_BUNDLE") and os.environ.get("QUAL_STATEMENT"),"set real-proof paths to run tampering checks")
class ProofTamperingTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
        self.bundle=self.root/"bundle";shutil.copytree(os.environ["QUAL_BUNDLE"],self.bundle)
        self.statement=q.load(os.environ["QUAL_STATEMENT"])
    def tearDown(self):self.tmp.cleanup()
    def reject(self):
        with self.assertRaises((ValueError,RuntimeError)):q.verify(self.statement,self.bundle,self.root/"verification")
    def test_external_input(self):self.statement["x0"]="1/10";self.reject()
    def test_external_parameter(self):self.statement["r"]="39/10";self.reject()
    def test_external_horizon(self):self.statement["horizon"]=10;self.reject()
    def test_external_threshold(self):self.statement["threshold"]="1/4";self.reject()
    def test_precision(self):
        p=self.bundle/"bundle.json";v=q.load(p);v["selected_precision"]=16;p.write_text(json.dumps(v));self.reject()
    def test_decision(self):
        p=self.bundle/"bundle.json";v=q.load(p);v["decision"]="False";p.write_text(json.dumps(v));self.reject()
    def test_radius(self):
        p=self.bundle/"attempt-0/statement.json";v=q.load(p);v["z0"][-3]="00"*32;p.write_text(json.dumps(v));self.reject()
    def test_initial_center(self):
        p=self.bundle/"attempt-0/statement.json";v=q.load(p);v["z0"][1]="00"*32;p.write_text(json.dumps(v));self.reject()
    def test_contract(self):
        p=self.bundle/"attempt-0/statement.json";v=q.load(p);v["input_contract"]="untrusted-contract";p.write_text(json.dumps(v));self.reject()
    def test_terminal(self):
        p=self.bundle/"attempt-0/terminal.json";v=q.load(p);v[-2]="01"+"00"*31;p.write_text(json.dumps(v));self.reject()
    def test_proof_bytes(self):
        p=self.bundle/"attempt-0/proof.bin";v=bytearray(p.read_bytes());v[len(v)//2]^=1;p.write_bytes(v);self.reject()

if __name__=="__main__":unittest.main()

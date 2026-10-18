#!/usr/bin/env python3
"""Unit tests, plus bounded native-component differential tests (no ZK proofs)."""
from copy import deepcopy
from fractions import Fraction as Q
import json
import unittest

from literature_artifacts import ROOT, P, accept_sridhar, native_sridhar, native_zkml, noir_check, rne
from literature_compare import (Bridge, error_bounds, hash_json, interval_decision,
                               make_inputs, own_check, own_witness, policies, table_bound)


class SemanticsTests(unittest.TestCase):
    def test_ties_even_and_negative(self):
        for x,n in [(Q(1,2),0),(Q(3,2),2),(Q(-1,2),0),(Q(-3,2),-2)]:
            self.assertEqual(rne(x/65536),n)

    def test_threshold_equality_is_asymmetric(self):
        self.assertIs(interval_decision(Q(1,2),Q(3,4),Q(1,2)),True)
        self.assertIsNone(interval_decision(Q(1,4),Q(1,2),Q(1,2)))
        self.assertIs(interval_decision(Q(1,4),Q(1,3),Q(1,2)),False)

    def test_errors_are_intervals(self):
        self.assertEqual(error_bounds(Q(1,2),["1/3","2/3"]),(Q(0),Q(1,6)))

    def test_exhaustive_not_grid_max(self):
        refs=[[str(Q(i,4)),str(Q(i,4))] for i in range(5)]
        values=[str(Q(i,4)) for i in range(5)]
        self.assertEqual(table_bound(values,refs,bits=2),Q(1,32))
        with self.assertRaises(ValueError): table_bound(values[:-1],refs,bits=2)

    def test_frozen_inputs_and_separate_cohorts(self):
        m=json.loads((ROOT/"experiments/literature_comparison_v1.json").read_text())
        a=make_inputs(m);self.assertEqual(a,make_inputs(m))
        self.assertEqual(sum(s["cohort"]=="random" for s in a["samples"]),1000)
        self.assertEqual(len(a["grid"]),1025)
        self.assertTrue(all(0<=Q(x)<=1 for x in a["scalars"]))


class NativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.inputs=[Q(0),Q(1,65536),Q(1,4),Q(1,2),Q(3,4),Q(1)]
        cls.profiles={m:native_sridhar(m,cls.inputs) for m in ("gl","poly","pade","pwl")}

    def test_all_honest_branches(self):
        for p in self.profiles.values():
            self.assertTrue(all(accept_sridhar(p,w) for w in p["witnesses"]))
            self.assertTrue(noir_check(p,p["witnesses"])["accepted"])

    def test_quantized_coefficients_known(self):
        p=self.profiles["pade"]["constants"]
        self.assertEqual(p["num"],[32768,16384,3277,273])
        self.assertEqual(p["denom"],[65536,0,6554,0])

    def test_reject_large_output_change_native_and_local(self):
        for m,p in self.profiles.items():
            w=deepcopy(p["witnesses"][2]);v=w["inp_struct"] if m=="gl" else w
            v["y"]=str(4*65536)
            self.assertFalse(accept_sridhar(p,w));self.assertFalse(noir_check(p,[w])["accepted"])

    def test_reject_gl_intermediate_and_input_mutation(self):
        p=self.profiles["gl"]
        for field in ("intermediate","input"):
            w=deepcopy(p["witnesses"][2])
            if field=="intermediate": w["wit_struct"]["mult_terms"][0]="0"
            else: w["inp_struct"]["x"]=str(65536*2)
            self.assertFalse(accept_sridhar(p,w));self.assertFalse(noir_check(p,[w])["accepted"])

    def test_polynomial_constant_power_not_silently_constrained(self):
        # Preserve upstream semantics, not an imagined canonical-power checker.
        p=self.profiles["poly"];w=deepcopy(p["witnesses"][2])
        w["intermediates"][0]=str(int(w["intermediates"][0])+16)
        w["y"]=str(int(w["y"])+8)
        self.assertTrue(accept_sridhar(p,w));self.assertTrue(noir_check(p,[w])["accepted"])

    def test_pwl_segment_membership(self):
        p=self.profiles["pwl"];w=deepcopy(p["witnesses"][0]);w["segment_index"]="7"
        self.assertFalse(accept_sridhar(p,w));self.assertFalse(noir_check(p,[w])["accepted"])

    def test_zkml_full_lookup_and_endpoints(self):
        table,_=native_zkml(16)
        self.assertEqual(len(table),65537);self.assertEqual(table[0],32768)
        self.assertTrue(all(a<=b for a,b in zip(table,table[1:])))
        self.assertNotEqual(table[-1],0)


class OurContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        b=Bridge();cls.contracts=b.call("contracts");b.close()

    def test_all_precisions_endpoints_and_reference(self):
        b=Bridge()
        try:
            for c in self.contracts:
                for x in (Q(0),Q(1,3),Q(1)):
                    w=own_witness(c,x);lo,hi=own_check(c,w,x)
                    ref=b.refs([x],512)[0]
                    self.assertLessEqual(lo,Q(ref[0]));self.assertGreaterEqual(hi,Q(ref[1]))
        finally: b.close()

    def test_bound_input_terms_and_profile_mutations(self):
        c=self.contracts[0];x=Q(1,3);w=own_witness(c,x)
        for key,value in [("radius","0"),("output","0"),("input","1/2"),("profile_hash","0")]:
            bad=dict(w);bad[key]=value
            with self.assertRaises(ValueError): own_check(c,bad,x)
        bad=deepcopy(w);bad["terms"][0]="0"
        with self.assertRaises(ValueError): own_check(c,bad,x)

    def test_domain_rejected(self):
        c=self.contracts[0];w=own_witness(c,Q(0));w["input"]="-1"
        with self.assertRaises(ValueError): own_check(c,w,Q(-1))

    def test_generation_byte_reproducible(self):
        c=self.contracts[0]
        self.assertEqual(own_witness(c,Q(1,3)),own_witness(c,Q(1,3)))


if __name__=="__main__": unittest.main()

"""Dependency-free checks of the independent Group B auditor."""
import copy
from fractions import Fraction as F
import hashlib
import json
import unittest

from qualification_group_b import bigint, check, contract_bound, errors, rational, rne


def integer(x):
    sign = (x > 0) - (x < 0)
    x = abs(x)
    limbs = []
    while x:
        limbs.append(x % 2**32)
        x >>= 32
    return [sign, limbs]


def encode(x):
    x = F(x)
    return [integer(x.numerator), integer(x.denominator)]


def fixture():
    # Legal rational contract, not falsely labelled as actual Legendre nodes.
    c = dict(version=1, format=dict(integer_bits=8, fractional_bits=16,
        signed=True, rounding="nearest-ties-to-even", overflow="reject"),
        nodes=[encode(F(-1,2)),encode(F(1,2))], weights=[encode(1),encode(1)],
        moment_degree=40, term_slack=encode(F(2,2**16)),sum_slack=encode(F(16,2**16)))
    cert = dict(contract_hash=hashlib.sha256(json.dumps(c,separators=(",", ":")).encode()).hexdigest(),
        canonical_input=encode(0),output=encode(F(1,2)),terms=[encode(0),encode(0)],claimed_radius=encode(1))
    return c, cert


class GroupBAuditTests(unittest.TestCase):
    def test_large_signed_rationals_roundtrip(self):
        for x in [F(0),F(-7,3),F(2**130+17,2**95+3)]:
            self.assertEqual(rational(encode(x)),x)
        with self.assertRaises(ValueError):
            bigint([1,[True]])

    def test_rne_signed_ties(self):
        for n, expected in [(1,0),(3,2),(5,2),(-1,0),(-3,-2),(-5,-2)]:
            self.assertEqual(rne(F(n,32),4),F(expected,16))

    def test_checked_bounds_are_not_observed_maxima(self):
        c, cert = fixture()
        lo, hi, parts = check(c,cert)
        self.assertLessEqual(lo,F(1,2))
        self.assertGreaterEqual(hi,F(1,2))
        self.assertGreater(parts["required_radius"],0)
        self.assertEqual(parts["required_radius"],sum(parts[k] for k in ("approximation","verification","input")))

    def test_tampered_witness_rejected(self):
        c, cert = fixture()
        for key, value in [("claimed_radius",encode(0)),("output",encode(F(3,4))),
                           ("output",encode(F(1,3))),("canonical_input",encode(2)),
                           ("terms",[encode(0)]),("terms",[encode(1),encode(0)]),
                           ("contract_hash","0"*64)]:
            bad=copy.deepcopy(cert)
            bad[key]=value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                check(c,bad)

    def test_bad_profile_rejected_even_with_rehashed_witness(self):
        c, cert = fixture()
        c["weights"][0]=encode(-1)
        cert["contract_hash"]=hashlib.sha256(json.dumps(c,separators=(",", ":")).encode()).hexdigest()
        with self.assertRaises(ValueError):
            check(c,cert)

    def test_error_interval_preserves_zero_and_uncertainty(self):
        ref=dict(lower=encode(F(49,100)),upper=encode(F(51,100)))
        self.assertEqual(errors(F(1,2),ref),(0,F(1,100)))
        self.assertEqual(errors(F(3,5),ref),(F(9,100),F(11,100)))

    def test_input_rounding_contributes_to_bound(self):
        c,cert=fixture()
        cert["canonical_input"]=encode(F(1,2**20))
        _,_,parts=check(c,cert)
        self.assertEqual(parts["input"],F(1,2**22))

    def test_different_accepted_outputs_are_allowed(self):
        c,cert=fixture()
        cert["output"]=encode(F(1,2)+F(1,2**16))
        a=2*rational(cert["output"])-1
        cert["terms"]=[encode(rne(a/(1+a*rational(r)),16)) for r in c["nodes"]]
        check(c,cert)


if __name__ == "__main__":
    unittest.main()

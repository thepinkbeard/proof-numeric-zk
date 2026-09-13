#!/usr/bin/env python3
"""Regression tests with synthetic fixtures, never experimental evidence."""
import csv
from fractions import Fraction
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

import qualification_group_a as group
import qualification_metrics as metrics
import plot_qualification as legacy


def row(family="cbd-local", lo="2/5", hi="3/5", ref_lo="9/20", ref_hi="23/50", threshold="1/2"):
    certified = family in metrics.CERTIFIED_FAMILIES
    decision = metrics.interval_decision(Fraction(lo), Fraction(hi), Fraction(threshold))
    reference = metrics.interval_decision(Fraction(ref_lo), Fraction(ref_hi), Fraction(threshold))
    resolved = None if reference == "Indeterminate" else reference == "True"
    return {"family": family, "lower": lo, "upper": hi, "threshold": threshold,
            "reference_lower": ref_lo, "reference_upper": ref_hi,
            "certified": certified, "decision": decision, "reference_decision": resolved,
            "predicate_flip": None if resolved is None or decision == "Indeterminate" else (decision == "True") != resolved}


def timing(policy="baseline-fixed-truncate", decision="False", certified=False):
    return {"policy": policy, "decision": decision, "certified_family": certified,
            "reference_decision": "True", "predicate_flip": decision == "False",
            "complete_wall_seconds": 2.0, "setup_excluded_seconds": 1.0,
            "primary_constraints": 100, "warmup": False, "round": 1,
            "fixed_precision": 32, "scenario": "boundary-10"}


class ClassificationTests(unittest.TestCase):
    def test_inclusive_lower_boundary_true(self):
        self.assertEqual(metrics.interval_decision(Fraction(1, 2), Fraction(3, 5), Fraction(1, 2)), "True")

    def test_equal_upper_boundary_is_ambiguous(self):
        self.assertEqual(metrics.interval_decision(Fraction(2, 5), Fraction(1, 2), Fraction(1, 2)), "Indeterminate")

    def test_exact_tie_zero_radius(self):
        r = row(lo="1/2", hi="1/2", ref_lo="1/2", ref_hi="1/2")
        self.assertEqual(metrics.validate_numeric_row(r)[0], "certified_same")

    def test_reversed_interval_rejected(self):
        with self.assertRaisesRegex(ValueError, "reversed"):
            metrics.interval_decision(Fraction(1), Fraction(0), Fraction(1, 2))

    def test_unresolved_reference_precedes_abstention(self):
        r = row(ref_lo="2/5", ref_hi="3/5")
        self.assertEqual(metrics.validate_numeric_row(r)[0], "reference_unresolved")

    def test_ambiguous_is_not_flip(self):
        self.assertEqual(metrics.validate_numeric_row(row())[0], "abstention")

    def test_plain_fixed_same_not_certified(self):
        r = row("fixed-rne", lo="2/5", hi="2/5")
        self.assertEqual(metrics.validate_numeric_row(r)[0], "observed_same")

    def test_plain_fixed_flip(self):
        r = row("fixed-rne", lo="3/5", hi="3/5")
        self.assertEqual(metrics.validate_numeric_row(r)[0], "observed_flip")

    def test_missing_metadata_rejected(self):
        r = row()
        del r["certified"]
        with self.assertRaisesRegex(ValueError, "metadata"):
            metrics.validate_numeric_row(r)

    def test_unbounded_family_cannot_promote_itself(self):
        r = row("fp32", lo="2/5", hi="2/5")
        r["certified"] = True
        with self.assertRaisesRegex(ValueError, "metadata"):
            metrics.validate_numeric_row(r)

    def test_forged_decision_rejected(self):
        r = row()
        r["decision"] = "True"
        with self.assertRaisesRegex(ValueError, "decision"):
            metrics.validate_numeric_row(r)

    def test_reference_midpoint_not_used(self):
        r = row("fixed-rne", lo="1/2", hi="1/2", ref_lo="49/100", ref_hi="3/5")
        r["reference_decision"] = True
        with self.assertRaisesRegex(ValueError, "decision"):
            metrics.validate_numeric_row(r)

    def test_certified_contradiction_rejected(self):
        r = row(lo="3/5", hi="7/10")
        with self.assertRaises(ValueError):
            metrics.validate_numeric_row(r)

    def test_false_flip_flag_rejected(self):
        r = row("fixed-rne", lo="3/5", hi="3/5")
        r["predicate_flip"] = False
        with self.assertRaisesRegex(ValueError, "predicate_flip"):
            metrics.validate_numeric_row(r)

    def test_exact_decision_below_float_resolution(self):
        tau = Fraction(1, 2)
        lo = tau - Fraction(1, 2**100)
        self.assertEqual(float(lo), float(tau))
        self.assertEqual(metrics.interval_decision(lo, lo, tau), "False")

    def test_profile_hash_matches_historical_schema(self):
        self.assertEqual(group.expected_profile("logistic", "7/2", None, "fixed-truncate", 16, "1/4"),
                         "5952cd3d2c8cd98115f012745580012e5761a5a909d6c362d39759b69faaa22c")

    def test_adaptive_group_does_not_split_selected_precision(self):
        a = {"family": "cdad-sequential", "precision": 16}
        b = {"family": "cdad-sequential", "precision": 64}
        self.assertEqual(group.method_key(a), group.method_key(b))


class TimingTests(unittest.TestCase):
    def test_valid_proof_wrong_predicate_is_not_certified(self):
        report = metrics.summarize_timing([timing() for _ in range(30)])
        self.assertEqual(report["certified_decisions"], 0)
        self.assertEqual(report["observed_decisions"], 30)
        self.assertEqual(report["flips"], 30)
        self.assertFalse(report["certified_family"])

    def test_missing_metadata_never_promotes(self):
        r = timing("fixed", "True", True)
        del r["certified_family"]
        report = metrics.summarize_timing([r])
        self.assertEqual(report["certified_decisions"], 0)
        self.assertEqual(report["observed_decisions"], 1)

    def test_certified_abstention_not_counted_definitive(self):
        report = metrics.summarize_timing([timing("fixed", "Indeterminate", True)])
        self.assertEqual(report["certified_decisions"], 0)
        self.assertEqual(report["observed_decisions"], 0)

    def test_certified_definitive_counted(self):
        self.assertEqual(metrics.summarize_timing([timing("fixed", "True", True)])["certified_decisions"], 1)

    def test_certified_wrong_rejected(self):
        with self.assertRaisesRegex(ValueError, "contradiction"):
            metrics.summarize_timing([timing("fixed", "False", True)])

    def test_false_certification_policy_rejected(self):
        with self.assertRaisesRegex(ValueError, "policy"):
            metrics.summarize_timing([timing(certified=True)])


def fixture(root):
    """Synthetic output enclosures for report plumbing, not logistic executions."""
    source = root / "source"
    source.mkdir()
    matrix = group.load(group.ROOT / "experiments/qualification_v1.json")
    config = group.load(group.DEFAULT_CONFIG)
    inputs = [{"id": "historical-0", "set": "historical", "value": "1/10", "seed": None},
              {"id": "synthetic-sample", "set": "sample", "value": "1/4", "seed": 1}]
    group.save(source / "matrix.json", matrix)
    group.save(source / "inputs.json", inputs)
    records = []
    for item in inputs:
        for workload, parameter, innovation, family, bits, horizon, threshold in sorted(group.expected_opportunities(matrix, config["horizons"])):
            bits = bits or 16
            certified = family in metrics.CERTIFIED_FAMILIES
            r = row(family, lo="2/5" if certified else "11/20", hi="3/5" if certified else "11/20", threshold=threshold)
            r.update(input_id=item["id"], input_set=item["set"], workload=workload,
                     parameter=parameter, innovation=innovation, precision=bits, horizon=horizon,
                     profile_hash=group.expected_profile(workload, parameter, innovation, family, bits, threshold),
                     evaluations=[bits], first_abstention=None, center_absolute_error=0.01, enclosure_width=0.2)
            records.append(r)
    with (source / "raw.jsonl").open("x") as stream:
        for r in records:
            stream.write(json.dumps(r) + "\n")
    columns = ["input_id", "workload", "parameter", "family", "precision", "iteration",
               "center", "lower", "upper", "reference_center", "error", "width"]
    with (source / "trajectories.csv").open("x", newline="") as stream:
        writer = csv.DictWriter(stream, columns)
        writer.writeheader()
        for parameter in config["trajectory"]["parameters"]:
            for family, bits in config["trajectory"]["methods"]:
                for t in range(config["trajectory"]["maximum_iteration"] + 1):
                    writer.writerow(dict(zip(columns, ["historical-0", "logistic", parameter, family, bits, t, .5, .4, .6, .455, .045, .2])))
    manifest = {"schema_version": 1, "status": "numeric-complete", "rejected": 0,
                "horizons": config["horizons"], "inputs": len(inputs), "rows": len(records), "full_matrix": False,
                "matrix_hash": group.digest(source / "matrix.json"), "inputs_hash": group.digest(source / "inputs.json"),
                "raw_hash": group.digest(source / "raw.jsonl")}
    group.save(source / "manifest.json", manifest)
    return source


class PipelineTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.source = fixture(self.root)

    def tearDown(self):
        self.temp.cleanup()

    def prepare(self, name="analysis"):
        out = self.root / name
        group.prepare(self.source, out, group.DEFAULT_CONFIG)
        return out

    def test_numeric_outputs_reproducible_byte_for_byte(self):
        a, b = self.prepare("a"), self.prepare("b")
        for filename in group.load(a / "manifest.json")["data_hashes"]:
            self.assertEqual((a / filename).read_bytes(), (b / filename).read_bytes(), filename)
        self.assertEqual(group.load(a / "manifest.json")["raw_records_validated"], 3540)

    def test_refuses_overwrite(self):
        out = self.prepare()
        before = (out / "manifest.json").read_bytes()
        with self.assertRaises(FileExistsError):
            group.prepare(self.source, out, group.DEFAULT_CONFIG)
        self.assertEqual(before, (out / "manifest.json").read_bytes())

    def test_bad_raw_hash_no_complete_manifest(self):
        p = self.source / "manifest.json"
        m = group.load(p)
        m["raw_hash"] = "0" * 64
        p.write_text(json.dumps(m))
        with self.assertRaisesRegex(ValueError, "raw source hash"):
            self.prepare()
        self.assertFalse((self.root / "analysis/manifest.json").exists())
        self.assertTrue((self.root / "analysis/failure.json").exists())

    def test_duplicate_opportunity_rejected(self):
        p = self.source / "raw.jsonl"
        lines = p.read_text().splitlines()
        lines[1] = lines[0]
        p.write_text("\n".join(lines) + "\n")
        with self.assertRaisesRegex(ValueError, "duplicate opportunity"):
            self.prepare()

    def test_tampered_derived_data_blocks_render(self):
        out = self.prepare()
        (out / "summary.json").write_text("[]")
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            group.render(out, self.root / "figures")
        self.assertFalse((self.root / "figures").exists())

    def test_legacy_report_has_dynamic_rounds_and_correct_certification(self):
        timings = self.root / "timing"
        timings.mkdir()
        group.save(timings / "manifest.json", {"status": "proof-campaign-complete", "rounds": 1})
        group.save(timings / "timing.json", [timing()])
        out = self.root / "legacy"
        argv = ["plot_qualification.py", "--input-dir", str(self.source), "--output-dir", str(out), "--timing-dir", str(timings)]
        with patch.object(sys, "argv", argv), patch.object(legacy, "figure"), patch.object(legacy.subprocess, "check_output", return_value="gnuplot test"):
            legacy.main()
        report = (out / "report.md").read_text()
        self.assertIn("com 1 rodadas", report)
        self.assertNotIn("com 30 rodadas", report)
        self.assertIn("| fixed-truncate F32 | 2.0000 | 1.0000 | 100 | 0 | 1 | 1 |", report)

    @unittest.skipUnless(shutil.which("gnuplot"), "gnuplot not installed")
    def test_all_figures_render_and_pdf_has_four_pages(self):
        from pypdf import PdfReader
        out = self.prepare()
        figures = self.root / "figures"
        pdf = self.root / "combined.pdf"
        group.render(out, figures, pdf)
        self.assertEqual(len(PdfReader(pdf).pages), 4)
        self.assertEqual(PdfReader(pdf).pages[0].extract_text().count("Entrada histórica"), 1)
        for number in range(1, 5):
            self.assertGreater((figures / f"{number:02d}.svg").stat().st_size, 1000)
        self.assertEqual(group.load(figures / "manifest.json")["status"], "figures-complete")
        with self.assertRaises(FileExistsError):
            group.render(out, figures, pdf)


if __name__ == "__main__":
    unittest.main()

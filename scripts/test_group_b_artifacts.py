"""Opt-in process/tamper tests. Set GROUP_B_RUN and optionally GROUP_B_REPLAY."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from qualification_group_b import audit

ROOT = Path(__file__).resolve().parents[1]
RUN = os.environ.get("GROUP_B_RUN")
REPLAY = os.environ.get("GROUP_B_REPLAY")
NUMERIC_FILES = ("matrix.json", "inputs.json", "contracts.json", "witnesses.json",
                 "decisions.json", "policies.json", "primitives.json", "search.json")


@unittest.skipUnless(RUN, "set GROUP_B_RUN to audit a generated pilot")
class GroupBArtifactsTests(unittest.TestCase):
    def setUp(self):
        self.run = Path(RUN).resolve()

    def mutate_copy(self, filename, mutation):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        copied = Path(temporary.name)/"run"
        copied.mkdir()
        manifest = json.loads((self.run/"manifest.json").read_text())
        for name in (*manifest["files"], "manifest.json"):
            shutil.copyfile(self.run/name, copied/name)
        path = copied/filename
        data = json.loads(path.read_text())
        mutation(data)
        path.write_text(json.dumps(data))
        # Rehash deliberately: test the numerical checker, not only a checksum.
        manifest["files"][filename] = hashlib.sha256(path.read_bytes()).hexdigest()
        (copied/"manifest.json").write_text(json.dumps(manifest))
        return copied

    def test_generated_artifacts_pass_independent_rational_audit(self):
        result = audit(self.run)[-1]
        self.assertEqual(result["scalar_certificates"], 816)
        self.assertEqual(result["composed_decisions"], 624)
        self.assertEqual(result["certified_contradictions"], 0)

    def test_rehashed_radius_tamper_rejected_by_both_implementations(self):
        copied = self.mutate_copy("witnesses.json", lambda rows: rows[0]["certificate"].update(claimed_radius=[[0, []],[1, [1]]]))
        with self.assertRaises(ValueError):
            audit(copied)
        process = subprocess.run([str(ROOT/"target/release/group-b-eval"), "verify", "--matrix",
            str(ROOT/"experiments/qualification_group_b_pilot_v1.json"), "--run-dir", str(copied)],
            capture_output=True, text=True, timeout=60)
        self.assertNotEqual(process.returncode, 0)
        self.assertIn("understated radius", process.stderr)

    def test_rehashed_non_legendre_constants_rejected(self):
        copied = self.mutate_copy("contracts.json", lambda rows: rows[0]["nodes"].__setitem__(0, [[0, []],[1, [1]]]))
        process = subprocess.run([str(ROOT/"target/release/group-b-eval"), "verify", "--matrix",
            str(ROOT/"experiments/qualification_group_b_pilot_v1.json"), "--run-dir", str(copied)],
            capture_output=True, text=True, timeout=60)
        self.assertNotEqual(process.returncode, 0)
        self.assertIn("frozen GL constants mismatch", process.stderr)

    def test_rehashed_decision_tamper_rejected(self):
        copied = self.mutate_copy("decisions.json", lambda rows: rows[0].update(decision="Indeterminate" if rows[0]["decision"] != "Indeterminate" else "True"))
        with self.assertRaises(ValueError):
            audit(copied)

    def test_missing_decision_rejected(self):
        copied = self.mutate_copy("decisions.json", lambda rows: rows.pop())
        with self.assertRaises(ValueError):
            audit(copied)

    def test_duplicate_witness_rejected(self):
        copied = self.mutate_copy("witnesses.json", lambda rows: rows.append(rows[0]))
        with self.assertRaises(ValueError):
            audit(copied)

    def test_generator_refuses_overwrite(self):
        before = (self.run/"manifest.json").read_bytes()
        process = subprocess.run([str(ROOT/"target/release/group-b-eval"), "generate", "--matrix",
            str(ROOT/"experiments/qualification_group_b_pilot_v1.json"), "--output-dir", str(self.run)],
            capture_output=True, timeout=60)
        self.assertNotEqual(process.returncode, 0)
        self.assertEqual(before, (self.run/"manifest.json").read_bytes())

    @unittest.skipUnless(REPLAY, "set GROUP_B_REPLAY for byte-for-byte comparison")
    def test_replay_numerics_are_byte_identical(self):
        for name in NUMERIC_FILES:
            with self.subTest(artifact=name):
                self.assertEqual((self.run/name).read_bytes(), (Path(REPLAY)/name).read_bytes())


if __name__ == "__main__":
    unittest.main()

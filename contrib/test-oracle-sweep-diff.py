#!/usr/bin/env python3
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "contrib" / "oracle-sweep-diff"
# Scratch output stays beside the repo, never in /tmp (AGENTS.md).
SCRATCH = ROOT.parent / "scratch"

ROW = '[[result]]\nscenario = "{}"\nrequest = "tree"\nverdict = "{}"\n'
SEED_MATCH = '[[result]]\nseed = {}\nverdict = "match"\n'
SEED_MISMATCH = '[[result]]\nseed = {}\nverdict = "mismatch"\nrequest = "tree"\n'


class OracleSweepDiffTest(unittest.TestCase):
    def sweep(self, root: Path, name: str, files: dict[str, str]) -> Path:
        directory = root / name
        directory.mkdir()
        for file, text in files.items():
            (directory / file).write_text(text)
        return directory

    def invoke(self, before: dict, after: dict):
        SCRATCH.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=SCRATCH) as tmp:
            a = self.sweep(Path(tmp), "a", before)
            b = self.sweep(Path(tmp), "b", after)
            return subprocess.run([SCRIPT, a, b], text=True, capture_output=True, check=False)

    def test_identical_sweeps_exit_zero(self):
        files = {"state.toml": ROW.format("x", "match"), "random-0.toml": SEED_MATCH.format(1)}
        result = self.invoke(files, files)
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn("lost: 0", result.stdout)

    def test_random_rows_key_by_seed_whatever_the_verdict(self):
        # A mismatching random row carries a request key; a matching one does not.
        result = self.invoke(
            {"random-0.toml": SEED_MATCH.format(1) + SEED_MISMATCH.format(2)},
            {"random-1.toml": SEED_MISMATCH.format(1) + SEED_MATCH.format(2)},
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("lost: 1\n  random 1", result.stdout)
        self.assertIn("gained: 1\n  random 2", result.stdout)
        self.assertIn("missing: 0", result.stdout)

    def test_missing_row_fails(self):
        result = self.invoke(
            {"state.toml": ROW.format("x", "match") + ROW.format("y", "match")},
            {"state.toml": ROW.format("x", "match")},
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("missing: 1\n  state y tree", result.stdout)


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "contrib" / "i3-suite-summary"
ORACLE = ROOT / ".cache" / "sway-ipc-oracle"


class I3SuiteSummaryTest(unittest.TestCase):
    def test_default_selects_and_discloses_snapshot(self):
        result = subprocess.run(
            [SCRIPT, ORACLE], text=True, capture_output=True, check=False
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("swayward snapshot: swayward-eb170906 (eb170906", result.stdout)

    def test_explicit_snapshot_is_supported(self):
        result = subprocess.run(
            [SCRIPT, ORACLE, "--swayward", "swayward-eb170906"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()

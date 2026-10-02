#!/usr/bin/env python3
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "contrib" / "targeted-oracle"
# Scratch output stays beside the repo, never in /tmp (AGENTS.md).
OUT = ROOT.parent / "scratch" / "targeted-oracle-test"


class TargetedOracleTest(unittest.TestCase):
    def invoke(self, *paths: str):
        return subprocess.run(
            [SCRIPT, "--out-dir", OUT, *paths], text=True, capture_output=True, check=False
        )

    def test_test_only_change_selects_nothing(self):
        result = self.invoke("src/tests/ipc/wire.rs")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("no oracle rows selected", result.stdout)

    def test_layout_change_selects_release_random_and_focused_rows(self):
        result = self.invoke("src/layout/mod.rs")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("target/release/swayward", result.stdout)
        self.assertIn("--seeds 0,50", result.stdout)
        self.assertIn("--scenario grouped_scratchpad_events", result.stdout)
        self.assertNotIn("target/debug", result.stdout)

    def test_unknown_production_path_fails_closed(self):
        result = self.invoke("src/new-production-module.rs")
        self.assertEqual(result.returncode, 2)
        self.assertIn("lack targeted oracle coverage", result.stderr)


if __name__ == "__main__":
    unittest.main()

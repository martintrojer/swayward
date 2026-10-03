#!/usr/bin/env python3
import subprocess
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "contrib" / "i3-suite-summary"
ORACLE = ROOT / ".cache" / "sway-ipc-oracle"


def pinned_snapshot() -> str:
    with (ORACLE / "pins.toml").open("rb") as file:
        return tomllib.load(file)["snapshot"]["swayward"]


class I3SuiteSummaryTest(unittest.TestCase):
    def test_default_selects_and_discloses_snapshot(self):
        result = subprocess.run(
            [SCRIPT, ORACLE], text=True, capture_output=True, check=False
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        snapshot = pinned_snapshot()
        commit = snapshot.removeprefix("swayward-")
        self.assertIn(f"swayward snapshot: {snapshot} ({commit}", result.stdout)

    def test_explicit_snapshot_is_supported(self):
        result = subprocess.run(
            [SCRIPT, ORACLE, "--swayward", pinned_snapshot()],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "contrib" / "targeted-oracle"
# Scratch output stays beside the repo, never in /tmp (AGENTS.md).
OUT = ROOT.parent / "scratch" / "targeted-oracle-test"


def load_module():
    loader = importlib.machinery.SourceFileLoader("targeted_oracle", str(SCRIPT))
    module = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
    loader.exec_module(module)
    return module


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

    def test_events_only_scenario_is_not_run_as_state(self):
        result = self.invoke("src/layout/mod.rs")
        state = next(line for line in result.stdout.splitlines() if "--corpus state" in line)
        self.assertNotIn("grouped_scratchpad_events", state)

    def test_baseline_comparison_separates_new_known_and_fixed(self):
        module = load_module()
        with tempfile.TemporaryDirectory(dir=OUT.parent) as tmp:
            now, before = Path(tmp, "now.toml"), Path(tmp, "before.toml")
            row = '[[result]]\nscenario = "{}"\nrequest = "tree"\nverdict = "{}"\n'
            before.write_text(row.format("a", "match") + row.format("b", "mismatch")
                              + row.format("c", "mismatch"))
            now.write_text(row.format("a", "mismatch") + row.format("b", "mismatch")
                           + row.format("c", "match"))
            new, known, fixed = module.compare(now, before)
            seed = '[[result]]\nseed = {}\nverdict = "{}"\n'
            before.write_text(seed.format(7, "match") + seed.format(8, "match"))
            now.write_text(seed.format(7, "mismatch") + seed.format(8, "match"))
            seeds = module.compare(now, before)
        self.assertEqual(seeds, ([(7,)], [], []))
        self.assertEqual(new, [("a", "tree")])
        self.assertEqual(known, [("b", "tree")])
        self.assertEqual(fixed, [("c", "tree")])

    def test_unknown_production_path_fails_closed(self):
        result = self.invoke("src/new-production-module.rs")
        self.assertEqual(result.returncode, 2)
        self.assertIn("lack targeted oracle coverage", result.stderr)


if __name__ == "__main__":
    unittest.main()

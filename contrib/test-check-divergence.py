#!/usr/bin/env python3
"""Tests for contrib/check-divergence against throwaway fixture repositories."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "check-divergence"
# /tmp is RAM-backed on the development hosts; keep fixtures on disk.
SCRATCH = os.environ.get("SWAYWARD_TEST_TMPDIR", "/var/tmp")

BODY = "".join(f"line {number}\n" for number in range(40))


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(
        ["git", "-C", root, "-c", "user.name=t", "-c", "user.email=t@t", *args],
        text=True,
    )


class CheckDivergenceTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="swayward-check-divergence.", dir=SCRATCH)
        self.root = Path(self.directory.name)
        git(self.root, "init", "-q")
        (self.root / "niri-config").mkdir()
        (self.root / "niri-config" / "lib.rs").write_text(BODY)
        (self.root / "kept.rs").write_text(BODY)
        git(self.root, "add", "-A")
        git(self.root, "commit", "-q", "-m", "base")
        base = git(self.root, "rev-parse", "HEAD").strip()
        (self.root / "docs" / "data").mkdir(parents=True)
        (self.root / "docs" / "FORK-BASE.md").write_text(f"commit {base}\n")
        self.ledger([])

    def tearDown(self):
        self.directory.cleanup()

    def ledger(self, paths: list[str]):
        directory = self.root / "docs" / "data" / "divergence"
        directory.mkdir(exist_ok=True)
        (directory / "_meta.toml").write_text("migrated_entries = 0\n")
        entry = directory / "test.toml"
        if not paths:
            entry.unlink(missing_ok=True)
            return
        entry.write_text(
            "[[edit]]\nmigrated = false\npaths = ["
            + ", ".join(f'"{path}"' for path in paths)
            + ']\nwhat = "x"\nwhy = "y"\n'
        )

    def move_and_edit(self):
        git(self.root, "mv", "niri-config", "swayward-config")
        path = self.root / "swayward-config" / "lib.rs"
        path.write_text(path.read_text() + "added line\n")

    def check(self) -> subprocess.CompletedProcess:
        return subprocess.run(
            [SCRIPT, self.root], capture_output=True, text=True, check=False
        )

    def test_unchanged_tree_passes(self):
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_edited_file_at_its_niri_path_needs_an_entry(self):
        (self.root / "kept.rs").write_text(BODY + "added line\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("  kept.rs\n", result.stderr)

    def test_pure_move_needs_no_entry(self):
        git(self.root, "mv", "niri-config", "swayward-config")
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_moved_and_edited_file_needs_an_entry(self):
        self.move_and_edit()
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("swayward-config/lib.rs (moved from niri-config/lib.rs", result.stderr)

    def test_moved_file_is_covered_by_its_new_path(self):
        self.move_and_edit()
        self.ledger(["swayward-config/lib.rs"])
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_moved_file_is_covered_by_its_old_path(self):
        self.move_and_edit()
        self.ledger(["niri-config/lib.rs"])
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_directory_entry_covers_files_beneath_it(self):
        self.move_and_edit()
        self.ledger(["swayward-config/"])
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_catch_all_entry_covers_nothing(self):
        self.move_and_edit()
        self.ledger(["*"])
        result = self.check()
        self.assertEqual(result.returncode, 1)

    def test_each_file_must_contain_exactly_one_entry(self):
        self.ledger(["kept.rs"])
        entry = self.root / "docs" / "data" / "divergence" / "test.toml"
        entry.write_text(entry.read_text() * 2)
        result = self.check()
        self.assertEqual(result.returncode, 2)
        self.assertIn("must contain exactly one [[edit]], found 2", result.stderr)


if __name__ == "__main__":
    unittest.main()

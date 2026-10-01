#!/usr/bin/env python3
"""Tests for contrib/tap-count, the counter coverage.toml's numbers come from."""

import subprocess
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("tap-count")


def count(log: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [SCRIPT], input=log, capture_output=True, text=True, check=False
    )


def report(log: str) -> str:
    result = count(log)
    lines = [line for line in result.stdout.splitlines() if line.startswith("report: ")]
    assert len(lines) == 1, result.stdout
    return lines[0].removeprefix("report: ")


class TapCountTest(unittest.TestCase):
    def test_plain_stream_counts_pass_fail_and_skip(self):
        log = "1..4\nok 1 - a\nnot ok 2 - b\nok 3 # skip no output\nok 4 - d\n"
        self.assertEqual(report(log), "2 pass; 1 skip; 1 fail")

    def test_unreached_assertions_are_reported_against_the_plan(self):
        self.assertEqual(report("1..3\nok 1 - a\n"), "1 of 3 reached; 1 pass")

    def test_indented_subtest_results_are_not_the_files_own(self):
        log = (
            "1..2\n"
            "    # Subtest: inner\n"
            "    ok 1 - x\n"
            "    not ok 2 - y\n"
            "    1..2\n"
            "not ok 1 - inner\n"
            "ok 2 - outer\n"
        )
        self.assertEqual(report(log), "1 pass; 1 fail")

    def test_a_panic_echo_of_the_stream_is_ignored(self):
        log = (
            "1..2\nok 1 - a\nnot ok 2 - b\n"
            "thread 'x' panicked at 'i3 test failed\nstdout:\n"
            "ok 1 - a\nnot ok 2 - b\n"
        )
        result = count(log)
        self.assertIn("panic echo, ignore", result.stdout)
        self.assertEqual(report(log), "1 pass; 1 fail")

    def test_todo_is_neither_a_pass_nor_a_failure(self):
        log = "1..3\nok 1 - a\nnot ok 2 - b # TODO later\nok 3 - c # todo done early\n"
        self.assertEqual(report(log), "1 pass; 2 todo")

    def test_escaped_hash_in_a_description_is_not_a_directive(self):
        # Test::More writes a '#' inside a test name as '\#'.
        log = "1..2\nok 1 - handles \\# skip marker\nnot ok 2 - mentions \\# TODO\n"
        self.assertEqual(report(log), "1 pass; 1 fail")

    def test_skip_must_follow_the_hash_as_a_word(self):
        log = "1..1\nok 1 - a # skipped-ish note\n"
        self.assertEqual(report(log), "1 pass")

    def test_skip_on_a_failure_is_still_a_failure(self):
        self.assertEqual(report("1..1\nnot ok 1 - a # skip nope\n"), "0 pass; 1 fail")

    def test_file_level_skip_plan(self):
        result = count("1..0 # SKIP needs X11\n")
        self.assertEqual(result.returncode, 0)
        self.assertIn("report: 0 reached", result.stdout)

    def test_missing_plan_is_reported(self):
        log = "ok 1 - a\nnot ok 2 - b\n"
        result = count(log)
        self.assertIn("plan: none emitted", result.stdout)
        self.assertEqual(report(log), "2 reached; 1 pass; 1 fail")

    def test_no_results_fails(self):
        self.assertEqual(count("nothing here\n").returncode, 1)


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).parents[1]
SCRIPT = Path(__file__).with_name("sync-github-wiki")


class WikiSyncTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_loader(
            "sync_github_wiki",
            importlib.machinery.SourceFileLoader("sync_github_wiki", str(SCRIPT)),
        )
        cls.sync = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.sync)

    def test_checked_in_wiki_has_no_broken_relative_links(self):
        publication = self.sync.publication(ROOT)
        self.assertEqual(self.sync.audit_links(publication), [])

    def test_link_audit_catches_a_missing_page(self):
        publication = self.sync.publication(ROOT)
        publication.pop("IPC_ORACLE_COVERAGE.md")
        errors = self.sync.audit_links(publication)
        self.assertTrue(any("IPC_ORACLE_COVERAGE.md" in error for error in errors), errors)

    def test_link_audit_checks_links_split_across_lines(self):
        publication = {"Home.md": b"[missing\npage](Missing.md)\n"}
        self.assertEqual(
            self.sync.audit_links(publication),
            ["Home.md:1: missing wiki target Missing.md"],
        )

    def test_publication_includes_assets_and_migration_pages(self):
        publication = self.sync.publication(ROOT)
        self.assertIn("Home.md", publication)
        self.assertIn("_Sidebar.md", publication)
        self.assertIn("_assets/icons/logo.svg", publication)
        self.assertIn("img/blur.png", publication)
        self.assertIn("SWAY_CONFIG_MIGRATION.md", publication)

    def test_writing_the_same_publication_twice_is_idempotent(self):
        publication = self.sync.publication(ROOT)
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            (destination / ".git").mkdir()
            self.sync.write_publication(destination, publication)
            first = {
                path.relative_to(destination).as_posix(): path.read_bytes()
                for path in destination.rglob("*")
                if path.is_file()
            }
            self.sync.write_publication(destination, publication)
            second = {
                path.relative_to(destination).as_posix(): path.read_bytes()
                for path in destination.rglob("*")
                if path.is_file()
            }
        self.assertEqual(first, second)


if __name__ == "__main__":
    unittest.main()

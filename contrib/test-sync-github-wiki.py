#!/usr/bin/env python3
import contextlib
import importlib.machinery
import importlib.util
import io
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

    def test_non_published_files_do_not_link_to_wiki_sources(self):
        self.assertEqual(self.sync.audit_source_link_styles(ROOT), [])

    def test_source_style_audit_catches_a_non_published_wiki_link(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "README.md").write_text("[page](docs/wiki/Page.md#section)\n")
            self.sync.run("git", "init", "--quiet", cwd=root)
            self.sync.run("git", "add", "README.md", cwd=root)
            errors = self.sync.audit_source_link_styles(root)
        self.assertEqual(
            errors,
            [
                "README.md:1: link to docs/wiki/Page.md#section must use "
                "https://github.com/martintrojer/swayward/wiki/Page#section"
            ],
        )

    def test_heading_audit_catches_a_body_h1_but_ignores_code_fences(self):
        publication = {
            "Page.md": b"```markdown\n# example\n```\n\n# Duplicate title\n",
            "README.md": b"# Not a published page\n",
        }
        self.assertEqual(
            self.sync.audit_headings(publication),
            ["Page.md:5: top-level heading duplicates the wiki page title"],
        )

    def test_link_audit_catches_a_missing_page(self):
        publication = self.sync.publication(ROOT)
        publication.pop("IPC_ORACLE_COVERAGE.md")
        errors = self.sync.audit_links(publication)
        # Page links are published without the `.md` extension, so the reported
        # target is the wiki page name rather than the tracked file name.
        self.assertTrue(any("IPC_ORACLE_COVERAGE" in error for error in errors), errors)

    def test_page_links_drop_the_md_extension(self):
        """`/wiki/Page.md` 302-redirects to raw markdown, so publish `Page`."""
        pages = {"Home.md", "Configuration:-Layout.md", "img/shot.png"}
        rewritten = self.sync.wiki_links(
            b"[a](Configuration:-Layout.md)\n"
            b"[b](./Configuration:-Layout.md#tab-indicator)\n"
            b"[c](img/shot.png)\n"
            b"[d](https://example.com/x.md)\n",
            pages,
        ).decode()
        # GitHub's Markdown renderer treats "Configuration:" as a URL scheme
        # and renders relative colon-page links as plain text. Publish those as
        # absolute wiki URLs so they remain clickable.
        wiki = "https://github.com/martintrojer/swayward/wiki/"
        self.assertIn(f"[a]({wiki}Configuration:-Layout)", rewritten)
        self.assertIn(f"[b]({wiki}Configuration:-Layout#tab-indicator)", rewritten)
        # Assets keep their extension, and external links are untouched.
        self.assertIn("[c](img/shot.png)", rewritten)
        self.assertIn("[d](https://example.com/x.md)", rewritten)

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

    def test_freshness_reports_every_kind_of_drift(self):
        errors = self.sync.freshness_errors(
            {"Home.md": b"new", "Added.md": b"x", "Same.md": b"s"},
            {"Home.md": b"old", "Removed.md": b"y", "Same.md": b"s"},
        )
        self.assertEqual(
            errors,
            [
                "Added.md: not published",
                "Removed.md: published but no longer in the repository",
                "Home.md: published content differs from the repository",
            ],
        )

    def test_freshness_accepts_an_identical_publication(self):
        files = self.sync.publication(ROOT)
        self.assertEqual(self.sync.freshness_errors(files, dict(files)), [])

    def test_published_files_ignores_the_git_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            checkout = Path(directory)
            (checkout / ".git").mkdir()
            (checkout / ".git" / "HEAD").write_text("ref: refs/heads/master\n")
            (checkout / "img").mkdir()
            (checkout / "img" / "a.png").write_bytes(b"\x89PNG")
            (checkout / "Home.md").write_bytes(b"# Home\n")
            self.assertEqual(
                self.sync.published_files(checkout),
                {"Home.md": b"# Home\n", "img/a.png": b"\x89PNG"},
            )

    def wiki_remote(self, directory, files):
        """Build a local git repository that stands in for the published wiki."""
        remote = Path(directory) / "wiki.git"
        remote.mkdir()
        for name, content in files.items():
            (remote / name).parent.mkdir(parents=True, exist_ok=True)
            (remote / name).write_bytes(content)
        self.sync.run("git", "init", "--quiet", "-b", "master", cwd=remote)
        self.sync.run("git", "add", "--all", cwd=remote)
        self.sync.run(
            "git",
            "-c", "user.name=t",
            "-c", "user.email=t@example.invalid",
            "commit", "--quiet", "-m", "publish",
            cwd=remote,
        )
        return f"file://{remote}"

    @contextlib.contextmanager
    def read_remote(self, url):
        original = self.sync.READ_REMOTE
        self.sync.READ_REMOTE = url
        try:
            yield
        finally:
            self.sync.READ_REMOTE = original

    def test_check_published_passes_against_a_matching_wiki(self):
        files = {"Home.md": b"# Home\n", "img/a.png": b"\x89PNG"}
        with tempfile.TemporaryDirectory() as directory:
            with self.read_remote(self.wiki_remote(directory, files)):
                out = io.StringIO()
                with contextlib.redirect_stdout(out):
                    status = self.sync.check_published(files)
        self.assertEqual(status, 0)
        self.assertIn("published wiki matches the repository", out.getvalue())

    def test_check_published_fails_on_a_stale_page(self):
        """The defect this gate exists for: the wiki still says the old thing."""
        with tempfile.TemporaryDirectory() as directory:
            published = {"Home.md": b"floating support is complete\n"}
            with self.read_remote(self.wiki_remote(directory, published)):
                err = io.StringIO()
                with contextlib.redirect_stderr(err):
                    status = self.sync.check_published(
                        {"Home.md": b"floating support is partial\n"}
                    )
        self.assertEqual(status, 1)
        self.assertIn(
            "Home.md: published content differs from the repository", err.getvalue()
        )
        self.assertIn("./contrib/sync-github-wiki", err.getvalue())

    def test_check_published_skips_cleanly_when_the_wiki_is_unreachable(self):
        """No network must not block a contributor, so offline is a skip."""
        with tempfile.TemporaryDirectory() as directory:
            unreachable = f"file://{Path(directory) / 'absent.git'}"
            with self.read_remote(unreachable):
                err = io.StringIO()
                with contextlib.redirect_stderr(err):
                    status = self.sync.check_published({"Home.md": b"x"})
        self.assertEqual(status, 0)
        self.assertIn("skipping the wiki freshness check", err.getvalue())


if __name__ == "__main__":
    unittest.main()

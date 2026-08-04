"""Tests for scripts/check_doc_links.py (relative markdown link check).

One case per skip class and rejection class, plus a positive control —
without it, a validator that errored on everything would pass every negative
test. The real trees are covered by `just check-doc-links` and CI, not
duplicated here.

unittest.TestCase so `python3 -m unittest discover -s scripts/tests` (wired
into `just check`) actually executes them; pytest runs them too.
"""

import pathlib
import subprocess
import sys
import tempfile
import unittest

REPO = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = REPO / "scripts" / "check_doc_links.py"
AGENTS_URL = "https://github.com/everruns/bashkit/blob/main/AGENTS.md"
AGENT_BADGE = "https://img.shields.io/badge/Repo-Agent%20Friendly-blue"


def run(root: pathlib.Path, *trees: str) -> subprocess.CompletedProcess:
    args = [sys.executable, str(SCRIPT), str(root)]
    for tree in trees:
        args += ["--tree", tree]
    return subprocess.run(args, capture_output=True, text=True)


class CheckDocLinksTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        (self.root / "docs").mkdir()
        (self.root / "guides").mkdir()
        (self.root / "guides" / "jq.md").write_text("# jq\n")
        self.addCleanup(self._tmp.cleanup)

    def write(self, body: str) -> None:
        (self.root / "docs" / "page.md").write_text(body)

    def check(self) -> subprocess.CompletedProcess:
        return run(self.root, "docs", "guides")

    def test_resolvable_link_accepted(self) -> None:
        self.write("See [jq](../guides/jq.md).\n")
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("doc links OK", result.stdout)

    def test_dangling_link_rejected(self) -> None:
        self.write("See [jq](jq.md).\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("docs/page.md:1: link target does not exist: jq.md", result.stderr)

    def test_anchor_is_stripped_before_resolving(self) -> None:
        self.write("See [jq](../guides/jq.md#usage) and [here](#top).\n")
        self.assertEqual(self.check().returncode, 0)

    def test_dangling_link_with_anchor_rejected(self) -> None:
        self.write("See [jq](jq.md#usage).\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("does not exist: jq.md#usage", result.stderr)

    def test_external_and_site_absolute_links_skipped(self) -> None:
        """Schemes, protocol-relative, and site-absolute paths name no file."""
        self.write(
            "[web](https://example.com/a.md) [proto](//example.com/a.md) "
            "[site](/builtins) [mail](mailto:x@example.com)\n"
        )
        self.assertEqual(self.check().returncode, 0)

    def test_rustdoc_intra_doc_links_skipped(self) -> None:
        """`crate::x` is a rustdoc path, not a file."""
        self.write("See [`threat_model`](crate::threat_model).\n")
        self.assertEqual(self.check().returncode, 0)

    def test_links_inside_code_are_not_links(self) -> None:
        """Prose about markdown, and shell examples containing `](`, are not links."""
        self.write(
            "Write it as `[Title](gone.md)`.\n\n```sh\necho '[x](gone.md)'\n```\n"
        )
        self.assertEqual(self.check().returncode, 0)

    def test_line_number_reported_after_fenced_block(self) -> None:
        """Blanked code keeps its newlines so later line numbers stay accurate."""
        self.write("```sh\necho hi\n```\n\nSee [jq](jq.md).\n")
        result = self.check()
        self.assertIn("docs/page.md:5:", result.stderr)

    def test_root_tree_is_not_recursive(self) -> None:
        """`.` scans root files only; nested trees are named explicitly."""
        (self.root / "README.md").write_text("See [jq](guides/jq.md).\n")
        self.write("See [jq](jq.md).\n")
        result = run(self.root, ".")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_tree_is_not_an_error(self) -> None:
        result = run(self.root, "docs", "nonexistent")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_root_rejected(self) -> None:
        result = run(self.root / "nope", "docs")
        self.assertEqual(result.returncode, 2)


class ReadmeLinksTest(unittest.TestCase):
    def test_agent_friendly_badge_targets_root_agents_file(self) -> None:
        readme = (REPO / "README.md").read_text()
        self.assertIn(f"[![Repo: Agent Friendly]({AGENT_BADGE})]({AGENTS_URL})", readme)


if __name__ == "__main__":
    unittest.main()

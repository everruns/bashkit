"""Tests for scripts/check_okf.py (OKF v0.2 bundle conformance check).

One case per rejection class, plus a positive control — without it, a
validator that errored on everything would pass every negative test.
Conformance of the real bundle is covered by `just check-okf` and CI, not
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
SCRIPT = REPO / "scripts" / "check_okf.py"

CONCEPT = """\
---
type: Subsystem Design
title: Widget
description: One sentence about the widget.
tags:
  - bashkit
---

# Widget
"""

ROOT_INDEX = """\
---
okf_version: "0.2"
---

# Bundle

* [Widget](widget.md) - One sentence about the widget.
"""

LOG = """\
# Bundle Update Log

## 2026-07-25
* **Creation**: Added [Widget](widget.md).
"""


def run(bundle: pathlib.Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(SCRIPT), str(bundle)],
        capture_output=True,
        text=True,
    )


class CheckOkfTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.bundle = pathlib.Path(self._tmp.name) / "bundle"
        self.bundle.mkdir()
        (self.bundle / "index.md").write_text(ROOT_INDEX)
        (self.bundle / "log.md").write_text(LOG)
        (self.bundle / "widget.md").write_text(CONCEPT)
        self.addCleanup(self._tmp.cleanup)

    def test_conformant_bundle_accepted(self) -> None:
        result = run(self.bundle)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("OKF v0.2 conformant", result.stdout)

    def test_missing_type_rejected(self) -> None:
        (self.bundle / "widget.md").write_text(
            CONCEPT.replace("type: Subsystem Design\n", "")
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("non-empty 'type'", result.stderr)

    def test_missing_frontmatter_rejected(self) -> None:
        (self.bundle / "widget.md").write_text("# Widget\n")
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("missing YAML frontmatter", result.stderr)

    def test_summary_instead_of_description_rejected(self) -> None:
        (self.bundle / "widget.md").write_text(
            CONCEPT.replace("description:", "summary:")
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("use 'description'", result.stderr)

    def test_index_frontmatter_rejected(self) -> None:
        (self.bundle / "index.md").write_text(
            ROOT_INDEX.replace('okf_version: "0.2"', "title: Bundle\nsummary: Nope.")
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("may not carry frontmatter keys", result.stderr)

    def test_log_heading_format_enforced(self) -> None:
        (self.bundle / "log.md").write_text("# Log\n\n## May 2026\n* Something.\n")
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("is not '## YYYY-MM-DD'", result.stderr)

    def test_dangling_link_rejected(self) -> None:
        (self.bundle / "widget.md").write_text(CONCEPT + "\nSee [gone](gone.md).\n")
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("link target does not exist: gone.md", result.stderr)

    def test_links_inside_code_are_not_links(self) -> None:
        """Prose about markdown, and shell examples containing `](`, are not links."""
        (self.bundle / "widget.md").write_text(
            CONCEPT
            + "\nFormat entries as `* [Title](path) - description`.\n"
            + '\n```console\n$ grep "a](*b)*c" file\n```\n'
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_external_links_not_checked(self) -> None:
        (self.bundle / "widget.md").write_text(
            CONCEPT + "\n[spec](https://example.com/a.md) and [anchor](#widget).\n"
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_unlisted_concept_rejected(self) -> None:
        (self.bundle / "orphan.md").write_text(CONCEPT)
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("orphan.md: not listed in index.md", result.stderr)


if __name__ == "__main__":
    unittest.main()

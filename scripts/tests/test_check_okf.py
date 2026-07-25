"""Tests for scripts/check_okf.py (OKF v0.2 bundle conformance check).

Written as unittest.TestCase so `python3 -m unittest discover -s scripts/tests`
(wired into `just check`) actually executes them; pytest runs them too.
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
        (self.bundle / "widget.md").write_text(CONCEPT)
        self.addCleanup(self._tmp.cleanup)

    def test_minimal_bundle_conforms(self) -> None:
        result = run(self.bundle)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("OKF v0.2 conformant", result.stdout)

    def test_repository_bundle_conforms(self) -> None:
        result = run(REPO / "knowledge")
        self.assertEqual(result.returncode, 0, result.stderr)

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

    def test_unterminated_frontmatter_rejected(self) -> None:
        (self.bundle / "widget.md").write_text("---\ntype: Widget\n\n# Widget\n")
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("unterminated frontmatter", result.stderr)

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

    def test_non_root_index_may_not_carry_okf_version(self) -> None:
        sub = self.bundle / "status"
        sub.mkdir()
        (sub / "index.md").write_text('---\nokf_version: "0.2"\n---\n\n# Status\n')
        (self.bundle / "index.md").write_text(
            ROOT_INDEX + "\n* [status/](status/) - Generated state.\n"
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("index.md may not carry frontmatter keys", result.stderr)

    def test_unlisted_concept_rejected(self) -> None:
        (self.bundle / "orphan.md").write_text(CONCEPT)
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("orphan.md: not listed in index.md", result.stderr)

    def test_subdirectory_without_index_rejected(self) -> None:
        (self.bundle / "status").mkdir()
        (self.bundle / "status" / "thing.md").write_text(CONCEPT)
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("subdirectory has no index.md", result.stderr)

    def test_log_heading_format_enforced(self) -> None:
        (self.bundle / "log.md").write_text("# Log\n\n## May 2026\n* Something.\n")
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("is not '## YYYY-MM-DD'", result.stderr)

    def test_log_frontmatter_rejected(self) -> None:
        (self.bundle / "log.md").write_text(
            "---\ntitle: Log\n---\n\n# Log\n\n## 2026-07-25\n* Something.\n"
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("log.md may not carry frontmatter", result.stderr)

    def test_valid_log_accepted(self) -> None:
        (self.bundle / "log.md").write_text("# Log\n\n## 2026-07-25\n* Something.\n")
        result = run(self.bundle)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_root_index_rejected(self) -> None:
        (self.bundle / "index.md").unlink()
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("bundle root index is missing", result.stderr)

    def test_missing_bundle_directory_errors(self) -> None:
        result = run(self.bundle / "nope")
        self.assertEqual(result.returncode, 2)
        self.assertIn("is not a directory", result.stderr)


if __name__ == "__main__":
    unittest.main()

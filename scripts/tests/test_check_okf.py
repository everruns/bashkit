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

## See also

- [Gadget](gadget.md) — the thing the widget drives
"""

# Two concepts, because a conformant bundle is a graph: the cross-link rule
# rejects a concept that links to nothing else.
GADGET = """\
---
type: Subsystem Design
title: Gadget
description: One sentence about the gadget.
---

# Gadget

See [Widget](widget.md).
"""

INVENTORY = """\
---
type: Generated Inventory
title: Parts
description: Generated list of parts.
resource: parts.json
generated:
  by: process:just-regen-parts
---

# Parts

Generated from [Widget](widget.md).
"""

ROOT_INDEX = """\
---
okf_version: "0.2"
---

# Bundle

* [Widget](widget.md) - One sentence about the widget.
* [Gadget](gadget.md) - One sentence about the gadget.
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
        (self.bundle / "gadget.md").write_text(GADGET)
        self.addCleanup(self._tmp.cleanup)

    def add_inventory(self, text: str = INVENTORY) -> None:
        """A Generated Inventory plus the artifact it describes, both listed."""
        (self.bundle / "parts.md").write_text(text)
        (self.bundle / "parts.json").write_text("{}\n")
        (self.bundle / "index.md").write_text(
            ROOT_INDEX + "* [Parts](parts.md) - Generated list of parts.\n"
        )

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

    def test_concept_without_cross_links_rejected(self) -> None:
        (self.bundle / "widget.md").write_text(CONCEPT.split("## See also")[0])
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("links to no other concept", result.stderr)

    def test_self_link_is_not_a_cross_link(self) -> None:
        (self.bundle / "widget.md").write_text(
            CONCEPT.replace("[Gadget](gadget.md)", "[Widget](widget.md)")
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("links to no other concept", result.stderr)

    def test_repository_path_reference_rejected(self) -> None:
        """The code-span form the 2026-07-26 restructure left dangling."""
        (self.bundle / "widget.md").write_text(
            CONCEPT + "\nSee `knowledge/gadget.md` for details.\n"
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("not as repository paths: knowledge/gadget.md", result.stderr)

    def test_generated_inventory_accepted(self) -> None:
        self.add_inventory()
        result = run(self.bundle)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_generated_inventory_without_producer_rejected(self) -> None:
        self.add_inventory(
            INVENTORY.replace("generated:\n  by: process:just-regen-parts\n", "")
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("must declare 'generated.by'", result.stderr)

    def test_generated_inventory_without_resource_rejected(self) -> None:
        self.add_inventory(INVENTORY.replace("resource: parts.json\n", ""))
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("must declare 'resource'", result.stderr)

    def test_missing_resource_artifact_rejected(self) -> None:
        self.add_inventory(INVENTORY.replace("parts.json", "gone.json"))
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("'resource' does not exist: gone.json", result.stderr)

    def test_non_actor_producer_rejected(self) -> None:
        self.add_inventory(INVENTORY.replace("process:just-regen-parts", "a script"))
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("is not an OKF actor", result.stderr)

    def test_illegal_lifecycle_values_rejected(self) -> None:
        (self.bundle / "widget.md").write_text(
            CONCEPT.replace("tags:", "status: retired\nstale_after: someday\ntags:")
        )
        result = run(self.bundle)
        self.assertEqual(result.returncode, 1)
        self.assertIn("'status' must be one of", result.stderr)
        self.assertIn("'stale_after' must be YYYY-MM-DD", result.stderr)


if __name__ == "__main__":
    unittest.main()

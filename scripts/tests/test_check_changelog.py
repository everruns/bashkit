"""Tests for scripts/check_changelog.py (convention C: Highlights + What's Changed)."""
import importlib.util
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "check_changelog", Path(__file__).resolve().parents[1] / "check_changelog.py")
mod = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(mod)

CLEAN = """\
# Changelog

## [Unreleased]

### Highlights

- **Security hardening.** Request sanitization
  ([#100](https://github.com/everruns/bashkit/pull/100)).
- **Faster builds.** Cached definitions reuse pre-built command objects on every run
  ([#101](https://github.com/everruns/bashkit/pull/101)).

### What's Changed

* something by @user in [#100](https://github.com/everruns/bashkit/pull/100)
"""


class CheckChangelogTests(unittest.TestCase):
    def test_clean_section_passes(self):
        self.assertEqual(mod.check_changelog(CLEAN), [])

    def test_orphan_bullet_fails(self):
        text = CLEAN.replace(
            "- **Faster builds.** Cached definitions reuse pre-built command objects on every run\n"
            "  ([#101](https://github.com/everruns/bashkit/pull/101)).",
            "- **Faster builds.** Cached definitions with no link.",
        )
        errs = mod.check_changelog(text)
        self.assertTrue(any("without PR link" in e for e in errs), errs)

    def test_verbatim_duplicate_fails(self):
        text = CLEAN.replace(
            "### What's Changed",
            "- **Faster builds.** Cached definitions reuse pre-built command objects on every run\n"
            "  ([#101](https://github.com/everruns/bashkit/pull/101)).\n\n### What's Changed",
        )
        errs = mod.check_changelog(text)
        self.assertTrue(any("duplicate bullet text" in e for e in errs), errs)

    def test_containment_duplicate_fails(self):
        text = CLEAN.replace(
            "### What's Changed",
            "- Cached definitions reuse pre-built command objects\n"
            "  ([#101](https://github.com/everruns/bashkit/pull/101)).\n\n### What's Changed",
        )
        # new bullet's text is contained in the Faster builds bullet -> duplicate
        errs = mod.check_changelog(text)
        self.assertTrue(any("duplicate bullet text" in e for e in errs), errs)

    def test_stale_pr_link_fails(self):
        text = CLEAN + "\n## [1.0.0] - 2026-01-01\n\n### Highlights\n\n" + \
            "- **Old work.** Did this before\n" + \
            "  ([#100](https://github.com/everruns/bashkit/pull/100)).\n"
        errs = mod.check_changelog(text)
        self.assertTrue(any("stale PR #100" in e for e in errs), errs)

    def test_extra_subsection_fails(self):
        text = CLEAN.replace(
            "### What's Changed",
            "### Fixed\n\n- A fix with link\n"
            "  ([#102](https://github.com/everruns/bashkit/pull/102)).\n\n### What's Changed",
        )
        errs = mod.check_changelog(text)
        self.assertTrue(any("unexpected subsection" in e for e in errs), errs)

    def test_malformed_link_fails(self):
        text = CLEAN.replace(
            "[#100](https://github.com/everruns/bashkit/pull/100)",
            "[#100](https://github.com/everruns/bashkit/pull/999)",
        )
        errs = mod.check_changelog(text)
        self.assertTrue(any("malformed link" in e for e in errs), errs)

    def test_whats_changed_exempt_from_link_rule(self):
        text = CLEAN.replace(
            "* something by @user in [#100](https://github.com/everruns/bashkit/pull/100)",
            "* something with no link at all",
        )
        self.assertEqual(mod.check_changelog(text), [])

    def test_scaffold_preserves_merge_order_drops_pr_less(self):
        subjects = ["feat: b (#12)", "fix: a (#3)", "chore: no pr here", "fix: a (#3)"]
        lines = mod.build_whats_changed(subjects)
        self.assertEqual(
            [ln for ln in lines if ln.startswith("* ")],
            ["* feat: b in [#12](https://github.com/everruns/bashkit/pull/12)",
             "* fix: a in [#3](https://github.com/everruns/bashkit/pull/3)"],
        )


if __name__ == "__main__":
    unittest.main()

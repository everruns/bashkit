#!/usr/bin/env python3
"""Validate relative markdown links in the repo's prose trees.

Why this exists: `docs/` and `crates/bashkit/docs/` render both on GitHub
(source-relative paths) and on bashkit.sh (flattened to `/docs/<slug>/`).
The site's `DOC_LINKS` map in `site/astro.config.mjs` resolves link targets by
*basename*, so a cross-tree link written as a bare `jq.md` still renders
correctly on the site while 404-ing for anyone reading the markdown on GitHub.
That asymmetry let 14 broken links accumulate unnoticed; this check closes it by
validating the source-relative form, which the site rewriter accepts too.

Not a replacement for `site/scripts/verify-doc-*.mjs` — those verify the built
routes. This one needs no build and runs in `just check`.

Skipped: URLs with a scheme (including rustdoc `crate::` intra-doc links),
protocol-relative and site-absolute paths, bare anchors, and anything inside
fenced or inline code.

No third-party dependencies, so the check runs anywhere `python3` does.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

# Trees whose markdown is browsed as source on GitHub. Root files are included
# because README/CONTRIBUTING link into them.
DEFAULT_ROOTS = ("docs", "crates/bashkit/docs", ".")

LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
# A scheme (`https:`, and rustdoc's `crate::`), protocol-relative, site-absolute,
# or a bare anchor — none of these name a file on disk.
EXTERNAL = re.compile(r"\A(?:[a-z][a-z0-9+.-]*:|//|/|#)")
# Fenced blocks first, then inline spans: prose about markdown (and shell
# examples containing `](`) must not be read as links.
CODE = re.compile(r"^```.*?^```|``.*?``|`[^`\n]*`", re.DOTALL | re.MULTILINE)


def strip_code(text: str) -> str:
    """Blank out fenced and inline code so link scanning ignores it."""
    return CODE.sub(lambda m: "\n" * m.group(0).count("\n"), text)


def check_file(path: pathlib.Path, rel: str, errors: list[str]) -> int:
    """Append an error for every relative link target that does not exist."""
    text = strip_code(path.read_text())
    checked = 0
    for match in LINK.finditer(text):
        target = match.group(1).strip()
        if not target or EXTERNAL.match(target):
            continue
        resolved = target.split("#", 1)[0]
        if not resolved:
            continue
        checked += 1
        if not (path.parent / resolved).exists():
            line = text[: match.start()].count("\n") + 1
            errors.append(f"{rel}:{line}: link target does not exist: {target}")
    return checked


def markdown_files(root: pathlib.Path, tree: str) -> list[pathlib.Path]:
    """Markdown under `tree`; non-recursive for `.` so only root files match."""
    base = root / tree
    if not base.is_dir():
        return []
    pattern = "*.md" if tree == "." else "**/*.md"
    return sorted(base.glob(pattern))


def check_trees(root: pathlib.Path, trees: tuple[str, ...]) -> tuple[list[str], dict[str, int]]:
    errors: list[str] = []
    stats = {"files": 0, "links": 0}
    for tree in trees:
        for path in markdown_files(root, tree):
            stats["files"] += 1
            stats["links"] += check_file(path, str(path.relative_to(root)), errors)
    return errors, stats


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", default=".", help="repository root")
    parser.add_argument(
        "--tree",
        action="append",
        dest="trees",
        help="tree to scan, relative to root (repeatable; defaults to docs trees + root files)",
    )
    args = parser.parse_args(argv)

    root = pathlib.Path(args.root)
    if not root.is_dir():
        print(f"not a directory: {root}", file=sys.stderr)
        return 2

    errors, stats = check_trees(root, tuple(args.trees or DEFAULT_ROOTS))
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        print(f"\n{len(errors)} broken relative link(s)", file=sys.stderr)
        return 1
    print(f"doc links OK: {stats['links']} relative links across {stats['files']} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Validate an Open Knowledge Format (OKF) v0.2 bundle.

Spec: https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md

Conformance rules enforced here (SPEC sections 4, 8, 9, 11, 12):

* every non-reserved `.md` file has a parseable YAML frontmatter block;
* every such frontmatter carries a non-empty `type`;
* reserved `index.md` carries no frontmatter, except the bundle-root one,
  which may carry only `okf_version`;
* reserved `log.md` carries no frontmatter and groups entries under
  `## YYYY-MM-DD` headings.

Bundle-local conventions also checked (see knowledge/knowledge-contract.md):
`title` and `description` are required, `description` is a single line, and
every concept and subdirectory is listed in its directory's `index.md`.

Trust and lifecycle metadata (SPEC sections 6, 7) is optional in OKF, but
where this bundle uses it the shape is enforced: a `Generated Inventory`
declares both its producing actor (`generated.by`) and the artifact it
describes (`resource`), actors follow the OKF actor convention, and
`status`/`stale_after` carry legal values.

Cross-linking (SPEC section 10): every concept must link to at least one
other concept, so the bundle is a graph rather than 30 disconnected files,
and bundle documents must be referenced as relative markdown links. A path
like `knowledge/foo.md` inside a code span is invisible to every link
checker — that is exactly how the 2026-07-26 restructure left ~30 dangling
references behind.

No third-party dependencies: the frontmatter subset used by this bundle is
parsed directly, so the check runs anywhere `python3` does.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

RESERVED = ("index.md", "log.md")
DATE_HEADING = re.compile(r"^## \d{4}-\d{2}-\d{2}\s*$")
LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
EXTERNAL = re.compile(r"\A(?:[a-z][a-z0-9+.-]*:|//|#)")
DATE = re.compile(r"\A\d{4}-\d{2}-\d{2}\Z")
# OKF actor convention: `<producer>/<version>`, `human:<id>`, `process:<id>`.
ACTOR = re.compile(r"\A(?:human:\S+|process:\S+|[^/\s]+/[^/\s]+)\Z")
STATUSES = ("draft", "stable", "deprecated")
# A bundle document addressed by its repository path instead of a relative
# link. Matched against the raw text, code spans included — the code-span form
# is the one that silently rots.
BUNDLE_PATH = re.compile(r"knowledge/[A-Za-z0-9_./-]+\.(?:md|json)")
# Fenced blocks first, then inline spans: prose about markdown (and shell
# examples containing `](`) must not be read as links. Missing this is how a
# bulk path rewrite silently corrupted a threat-model regex payload.
CODE = re.compile(r"^```.*?^```|``.*?``|`[^`\n]*`", re.DOTALL | re.MULTILINE)


def strip_code(text: str) -> str:
    """Blank out fenced and inline code so link scanning ignores it."""
    return CODE.sub(lambda m: "\n" * m.group(0).count("\n"), text)


def split_frontmatter(text: str) -> tuple[str | None, str]:
    """Return (frontmatter, body). Frontmatter is None when absent."""
    if not text.startswith("---\n"):
        return None, text
    end = text.find("\n---\n", 3)
    if end == -1:
        raise ValueError("unterminated frontmatter block")
    return text[4:end], text[end + 5 :]


def parse_frontmatter(fm: str) -> dict[str, object]:
    """Parse the flat-scalar + list-of-scalars + one-level-mapping YAML subset."""
    data: dict[str, object] = {}
    key: str | None = None
    for lineno, raw in enumerate(fm.splitlines(), start=2):
        line = raw.rstrip()
        if not line or line.lstrip().startswith("#"):
            continue
        if line.startswith("  "):
            if key is None:
                raise ValueError(f"line {lineno}: indented entry without a key")
            item = line.strip()
            if item.startswith("- "):
                existing = data.get(key)
                items = existing if isinstance(existing, list) else []
                data[key] = items + [item[2:].strip()]
            elif ":" in item:
                nested = data.get(key)
                if not isinstance(nested, dict):
                    nested = {}
                    data[key] = nested
                sub, _, val = item.partition(":")
                nested[sub.strip()] = val.strip()
            else:
                raise ValueError(f"line {lineno}: unparseable entry {item!r}")
            continue
        if ":" not in line:
            raise ValueError(f"line {lineno}: unparseable key line {line!r}")
        key, _, value = line.partition(":")
        key = key.strip()
        value = value.strip()
        data[key] = value if value else ""
    return data


def index_targets(path: pathlib.Path) -> set[str]:
    if not path.exists():
        return set()
    _, body = split_frontmatter(path.read_text())
    return {t.split("#", 1)[0].rstrip("/") for t in LINK.findall(strip_code(body))}


def check_links(path: pathlib.Path, rel: str, errors: list[str]) -> None:
    """Every bundle-relative link must resolve to a file that exists."""
    _, body = split_frontmatter(path.read_text())
    for target in LINK.findall(strip_code(body)):
        if EXTERNAL.match(target):
            continue
        resolved = target.split("#", 1)[0]
        if not resolved:
            continue
        if not (path.parent / resolved).exists():
            errors.append(f"{rel}: link target does not exist: {target}")


def check_concept(path: pathlib.Path, rel: str, errors: list[str]) -> None:
    text = path.read_text()
    fm_text, body = split_frontmatter(text)
    if fm_text is None:
        errors.append(f"{rel}: missing YAML frontmatter block")
        return
    fm = parse_frontmatter(fm_text)
    if not fm.get("type"):
        errors.append(f"{rel}: frontmatter must contain a non-empty 'type'")
    for field in ("title", "description"):
        if not fm.get(field):
            errors.append(f"{rel}: frontmatter must contain a non-empty '{field}'")
    if "summary" in fm:
        errors.append(f"{rel}: 'summary' is not an OKF field — use 'description'")
    check_trust(path, rel, fm, errors)
    check_cross_links(path, rel, body, errors)


def check_trust(
    path: pathlib.Path, rel: str, fm: dict[str, object], errors: list[str]
) -> None:
    """Trust and lifecycle metadata is optional; where present it must be sound.

    A generated concept that does not say who produced it, or what artifact it
    describes, is indistinguishable from a hand-written one — which is the one
    distinction a reader needs before trusting it over the source.
    """
    generated = fm.get("generated")
    resource = fm.get("resource")
    if fm.get("type") == "Generated Inventory":
        if not isinstance(generated, dict) or not generated.get("by"):
            errors.append(
                f"{rel}: a 'Generated Inventory' must declare 'generated.by' "
                f"(the producing actor)"
            )
        if not resource:
            errors.append(
                f"{rel}: a 'Generated Inventory' must declare 'resource' "
                f"(the artifact it describes)"
            )
    if isinstance(generated, dict):
        actor = generated.get("by")
        if actor and not ACTOR.match(str(actor)):
            errors.append(
                f"{rel}: 'generated.by' is not an OKF actor "
                f"('<producer>/<version>', 'human:<id>', 'process:<id>'): {actor!r}"
            )
    if isinstance(resource, str) and resource and not EXTERNAL.match(resource):
        if not (path.parent / resource).exists():
            errors.append(f"{rel}: 'resource' does not exist: {resource}")
    status = fm.get("status")
    if status and status not in STATUSES:
        errors.append(f"{rel}: 'status' must be one of {STATUSES}, got {status!r}")
    stale_after = fm.get("stale_after")
    if stale_after and not DATE.match(str(stale_after)):
        errors.append(f"{rel}: 'stale_after' must be YYYY-MM-DD, got {stale_after!r}")


def check_bundle_paths(rel: str, text: str, errors: list[str]) -> None:
    """Bundle documents are addressed as relative links, never as repo paths."""
    for match in sorted(set(BUNDLE_PATH.findall(text))):
        errors.append(
            f"{rel}: reference bundle documents as relative markdown links, "
            f"not as repository paths: {match}"
        )


def check_cross_links(
    path: pathlib.Path, rel: str, body: str, errors: list[str]
) -> None:
    """Every concept links to at least one other concept."""
    for target in LINK.findall(strip_code(body)):
        if EXTERNAL.match(target):
            continue
        resolved = (path.parent / target.split("#", 1)[0]).resolve()
        if (
            resolved.suffix == ".md"
            and resolved.name not in RESERVED
            and resolved != path.resolve()
            and resolved.exists()
        ):
            return
    errors.append(
        f"{rel}: links to no other concept — add a 'See also' section so the "
        f"bundle stays traversable"
    )


def check_index(path: pathlib.Path, rel: str, is_root: bool, errors: list[str]) -> None:
    fm_text, body = split_frontmatter(path.read_text())
    if fm_text is not None:
        keys = set(parse_frontmatter(fm_text))
        allowed = {"okf_version"} if is_root else set()
        extra = sorted(keys - allowed)
        if extra:
            errors.append(
                f"{rel}: index.md may not carry frontmatter keys {extra} "
                f"(allowed here: {sorted(allowed) or 'none'})"
            )
    if not body.strip():
        errors.append(f"{rel}: index.md body is empty")


def check_log(path: pathlib.Path, rel: str, errors: list[str]) -> None:
    fm_text, body = split_frontmatter(path.read_text())
    if fm_text is not None:
        errors.append(f"{rel}: log.md may not carry frontmatter")
    headings = [ln for ln in body.splitlines() if ln.startswith("## ")]
    if not headings:
        errors.append(f"{rel}: log.md needs at least one '## YYYY-MM-DD' heading")
    for heading in headings:
        if not DATE_HEADING.match(heading):
            errors.append(f"{rel}: log heading {heading!r} is not '## YYYY-MM-DD'")


def check_bundle(root: pathlib.Path) -> tuple[list[str], dict[str, int]]:
    errors: list[str] = []
    counts = {"concepts": 0, "indexes": 0, "logs": 0}
    if not (root / "index.md").exists():
        errors.append("index.md: bundle root index is missing")

    for directory in sorted(p for p in root.rglob("*") if p.is_dir()) + [root]:
        listed = index_targets(directory / "index.md")
        for child in sorted(directory.iterdir()):
            rel = child.relative_to(root).as_posix()
            if child.is_dir():
                if not (child / "index.md").exists():
                    errors.append(f"{rel}/: subdirectory has no index.md")
                if child.name not in listed:
                    errors.append(f"{rel}/: not listed in {directory.name}/index.md")
                continue
            if child.suffix != ".md":
                continue
            if child.name in RESERVED:
                continue
            if child.name not in listed:
                errors.append(
                    f"{rel}: not listed in "
                    f"{(directory / 'index.md').relative_to(root).as_posix()}"
                )

    for path in sorted(root.rglob("*.md")):
        rel = path.relative_to(root).as_posix()
        try:
            if path.name == "index.md":
                counts["indexes"] += 1
                check_index(path, rel, is_root=path.parent == root, errors=errors)
            elif path.name == "log.md":
                counts["logs"] += 1
                check_log(path, rel, errors)
            else:
                counts["concepts"] += 1
                check_concept(path, rel, errors)
            check_links(path, rel, errors)
            check_bundle_paths(rel, path.read_text(), errors)
        except ValueError as exc:
            errors.append(f"{rel}: {exc}")

    return errors, counts


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", nargs="?", default="knowledge", type=pathlib.Path)
    args = parser.parse_args(argv)

    root = args.bundle
    if not root.is_dir():
        print(f"error: {root} is not a directory", file=sys.stderr)
        return 2

    errors, counts = check_bundle(root)
    if errors:
        print(f"{root}: {len(errors)} OKF conformance error(s)", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        return 1

    print(
        f"{root}: OKF v0.2 conformant ({counts['concepts']} concepts, "
        f"{counts['indexes']} index files, {counts['logs']} log file"
        f"{'s' if counts['logs'] != 1 else ''})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

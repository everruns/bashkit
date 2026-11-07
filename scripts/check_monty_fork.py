#!/usr/bin/env python3
"""Check that the Monty forks differ from upstream only where marked.

crates/bashkit-monty and crates/bashkit-monty-types are copies of Monty 1.4.0
from crates.io with a few local edits, each marked with a `bashkit fork:`
comment. This compares every file in the forks with the pinned upstream
`.crate` and fails on any changed region that has no marker in or just above
it, and on files that exist only in the fork.

Decision: the upstream archives are pinned by SHA-256 so the check cannot be
satisfied by a swapped download; bump UPSTREAM together with a re-sync.

Usage:
  scripts/check_monty_fork.py                    # download pinned upstream
  scripts/check_monty_fork.py --upstream-dir D   # D/<crate>-<version>/ already extracted
"""

from __future__ import annotations

import argparse
import difflib
import hashlib
import io
import re
import sys
import tarfile
import tempfile
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MARKER = 'bashkit fork:'
# Plain lines above a changed region that may hold its marker comment. The
# comment/attribute block attached to the changed item is searched in full on
# top of this, however long it is, see `marker_window`.
MARKER_WINDOW = 3
VERSION = '1.4.0'
UPSTREAM = {
    # fork directory: (crates.io name, sha256 of the .crate)
    'bashkit-monty': (
        'monty',
        'b7d354901f6d6655e6bc3826f31c035417025253d7c3f7b3ef1519f681dd3469',
    ),
    'bashkit-monty-types': (
        'monty-types',
        '560e0f529ceb10575efb25dabc95d41e9de0f5036fbb46f02c2ac03d83dc89e3',
    ),
}
# Fork-only packaging files, not upstream source. Cargo.toml is not diffed as
# text (the fork's is hand-written, upstream's is cargo-normalized); its
# dependencies and features are compared instead, see `manifest_errors`.
FORK_ONLY = {'Cargo.toml', 'LICENSE', 'rustfmt.toml'}
# Upstream files the forks deliberately leave out: upstream tests (not
# vendored) and packaging that cargo regenerates. Any other upstream file
# missing from a fork is an error.
UPSTREAM_DROPPED = ('tests/', 'test_cases/', 'Cargo.lock', 'Cargo.toml.orig', '.cargo_vcs_info.json')
# Dependency tables compared between the fork and upstream manifests.
# dev-dependencies are skipped with the tests that use them.
DEP_TABLES = ('dependencies', 'build-dependencies')
# Dependencies the forks rename on purpose: upstream name -> fork package.
# Their version follows the fork, so only features/optionality are compared.
RENAMED_DEPS = {'monty-types': 'bashkit-monty-types'}


def fetch(name: str, sha256: str, dest: Path) -> Path:
    url = f'https://static.crates.io/crates/{name}/{name}-{VERSION}.crate'
    req = urllib.request.Request(url, headers={'User-Agent': 'bashkit-fork-check'})
    with urllib.request.urlopen(req, timeout=60) as resp:
        data = resp.read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        raise SystemExit(f'{name}-{VERSION}.crate sha256 {got} != pinned {sha256}')
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as tar:
        tar.extractall(dest, filter='data')
    return dest / f'{name}-{VERSION}'


def is_attached_comment(line: str) -> bool:
    """A comment or attribute line, which in Rust binds to the item below it."""
    stripped = line.lstrip()
    return stripped.startswith('//') or stripped.startswith('#[') or stripped.startswith('#![')


def marker_window(fork: list[str], j1: int, j2: int) -> list[str]:
    """Fork lines that may carry the marker for the changed region `j1:j2`.

    Decision: the contiguous comment/attribute block above the region is
    searched in full, not just `MARKER_WINDOW` lines. A fork note here is
    usually several lines of rationale and the marker sits on its first line,
    so a fixed window reads the tail of the note and misses the marker. The
    block stops at the first line of code, so one item's note cannot mark the
    next item's drift.
    """
    start = j1
    while start > 0 and is_attached_comment(fork[start - 1]):
        start -= 1
    return fork[min(start, max(0, j1 - MARKER_WINDOW)) : j2]


# `name = ...` at the start of a line: a dependency entry in a `[dependencies]`
# style table. Quoted keys are accepted because TOML allows them.
TOML_ENTRY = re.compile(r'^\s*(?:"(?P<quoted>[^"]+)"|(?P<bare>[A-Za-z0-9_.+-]+))\s*=')
TOML_TABLE = re.compile(r'^\s*\[\s*([^\]]+?)\s*\]\s*$')


def is_toml_comment(line: str) -> bool:
    """A comment line, which in TOML binds to the entry below it."""
    return line.lstrip().startswith('#')


def toml_marker_window(lines: list[str], j: int) -> list[str]:
    """Lines that may carry the marker for the entry declared at line `j`.

    The contiguous comment block directly above the entry, in full, plus the
    entry's own line (a short note often sits at the end of it).

    Decision: no `MARKER_WINDOW` fallback here, unlike source. A dependency
    table is one entry per line, so "a few plain lines above" would reach over
    the preceding dependencies and let one note excuse several bumps. The
    comment block stops at the first non-comment line instead.
    """
    start = j
    while start > 0 and is_toml_comment(lines[start - 1]):
        start -= 1
    return lines[start : j + 1]


def marked_deps(fork_dir: Path) -> set[str]:
    """Dependency names in the fork manifest carrying a `bashkit fork:` marker.

    Decision: a *version* divergence may be marked, the way a source edit is.
    A dependency the fork shares with bashkit's own crates is declared twice,
    once here and once in the workspace, so pinning this one to upstream puts
    two majors of the same crate in the graph; the alternative, telling
    Dependabot to leave the name alone, would freeze it for bashkit too
    (`serde`, `chrono`, `sha2` among them). Adding or dropping a dependency
    stays unmarkable: re-sync cannot reconcile a crate upstream does not have.
    """
    marked = set()
    lines = (fork_dir / 'Cargo.toml').read_text(encoding='utf-8').splitlines()
    table = None
    for j, line in enumerate(lines):
        header = TOML_TABLE.match(line)
        if header:
            table = header.group(1)
            # `[dependencies.serde]` marks that one dependency.
            parts = [p.strip() for p in table.split('.')]
            if len(parts) >= 2 and parts[-2] in DEP_TABLES:
                if any(MARKER in w for w in toml_marker_window(lines, j)):
                    marked.add(parts[-1])
            continue
        if table is None:
            continue
        # Only a dependency table's entries; `[features]` entries look the same.
        if table.split('.')[-1] not in DEP_TABLES:
            continue
        entry = TOML_ENTRY.match(line)
        if not entry:
            continue
        name = entry.group('quoted') or entry.group('bare')
        if any(MARKER in w for w in toml_marker_window(lines, j)):
            marked.add(name)
    return marked


def unmarked_hunks(upstream: list[str], fork: list[str]) -> list[int]:
    """1-based fork line numbers of changed regions with no marker nearby."""
    bad = []
    matcher = difflib.SequenceMatcher(a=upstream, b=fork, autojunk=False)
    for tag, _i1, _i2, j1, j2 in matcher.get_opcodes():
        if tag == 'equal':
            continue
        if not any(MARKER in line for line in marker_window(fork, j1, j2)):
            bad.append(j1 + 1)
    return bad


def read_lines(path: Path) -> list[str] | None:
    """Text lines, or None for a file that is not UTF-8 (compared as bytes)."""
    try:
        return path.read_text(encoding='utf-8').splitlines()
    except UnicodeDecodeError:
        return None


def dep_tables(manifest: dict) -> dict[str, dict]:
    tables = {t: manifest.get(t, {}) for t in DEP_TABLES}
    for cfg, target in manifest.get('target', {}).items():
        for t in DEP_TABLES:
            if t in target:
                tables[f'target.{cfg}.{t}'] = target[t]
    return {name: deps for name, deps in tables.items() if deps}


def dep_spec(name: str, spec: dict | str) -> tuple:
    if isinstance(spec, str):
        spec = {'version': spec}
    version = None if name in RENAMED_DEPS else spec.get('version')
    return (
        version,
        tuple(sorted(spec.get('features', []))),
        spec.get('default-features', spec.get('default_features', True)),
        spec.get('optional', False),
    )


def manifest_errors(fork_dir: Path, upstream_dir: Path) -> list[str]:
    """Dependencies and features must match upstream's normalized manifest."""
    fork = tomllib.loads((fork_dir / 'Cargo.toml').read_text(encoding='utf-8'))
    upstream = tomllib.loads((upstream_dir / 'Cargo.toml').read_text(encoding='utf-8'))
    where = f'{fork_dir.name}/Cargo.toml'
    errors = []
    marked = marked_deps(fork_dir)
    fork_tables, up_tables = dep_tables(fork), dep_tables(upstream)
    for table in sorted(set(fork_tables) | set(up_tables)):
        fork_deps, up_deps = fork_tables.get(table, {}), up_tables.get(table, {})
        for name in sorted(set(fork_deps) | set(up_deps)):
            if name not in up_deps:
                errors.append(f'{where}: [{table}] adds `{name}`, not in upstream')
            elif name not in fork_deps:
                errors.append(f'{where}: [{table}] drops upstream `{name}`')
            else:
                fork_spec, up_spec = fork_deps[name], up_deps[name]
                renamed = RENAMED_DEPS.get(name)
                if renamed and (not isinstance(fork_spec, dict) or fork_spec.get('package') != renamed):
                    errors.append(f'{where}: [{table}] `{name}` must be package `{renamed}`')
                differs = dep_spec(name, fork_spec) != dep_spec(name, up_spec)
                if differs and name not in marked:
                    errors.append(f'{where}: [{table}] `{name}` differs from upstream {up_spec!r}')
                elif not differs and name in marked and name not in RENAMED_DEPS:
                    # A note left behind by a re-sync would silently excuse the
                    # next bump of this crate.
                    errors.append(
                        f'{where}: [{table}] `{name}` matches upstream but has a '
                        f'`{MARKER}` marker; drop the marker'
                    )
    if fork.get('features', {}) != upstream.get('features', {}):
        errors.append(f'{where}: [features] differ from upstream {upstream.get("features", {})!r}')
    return errors


def check_crate(fork_dir: Path, upstream_dir: Path) -> list[str]:
    errors = manifest_errors(fork_dir, upstream_dir)
    for path in sorted(upstream_dir.rglob('*')):
        rel = path.relative_to(upstream_dir).as_posix()
        if path.is_file() and not rel.startswith(UPSTREAM_DROPPED) and not (fork_dir / rel).is_file():
            errors.append(f'{fork_dir.name}/{rel}: upstream file missing from the fork')
    for path in sorted(fork_dir.rglob('*')):
        if not path.is_file():
            continue
        rel = path.relative_to(fork_dir)
        if rel.as_posix() in FORK_ONLY:
            continue
        up = upstream_dir / rel
        fork_lines = read_lines(path)
        if not up.is_file():
            if fork_lines is None or not any(MARKER in line for line in fork_lines[:MARKER_WINDOW]):
                errors.append(f'{fork_dir.name}/{rel}: not in upstream and has no `{MARKER}` header')
            continue
        up_lines = read_lines(up)
        if fork_lines is None or up_lines is None:
            if path.read_bytes() != up.read_bytes():
                errors.append(f'{fork_dir.name}/{rel}: binary file differs from upstream')
            continue
        for line in unmarked_hunks(up_lines, fork_lines):
            errors.append(f'{fork_dir.name}/{rel}:{line}: differs from upstream without a `{MARKER}` marker')
    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--upstream-dir', type=Path)
    parser.add_argument('--root', type=Path, default=ROOT)
    args = parser.parse_args(argv)

    errors = []
    with tempfile.TemporaryDirectory() as tmp:
        for fork, (name, sha256) in UPSTREAM.items():
            if args.upstream_dir:
                upstream = args.upstream_dir / f'{name}-{VERSION}'
            else:
                upstream = fetch(name, sha256, Path(tmp))
            errors += check_crate(args.root / 'crates' / fork, upstream)

    for e in errors:
        print(e, file=sys.stderr)
    if errors:
        print(f'{len(errors)} fork drift error(s); mark source edits with `// {MARKER} <why>`', file=sys.stderr)
        return 1
    print(f'Monty forks match upstream {VERSION} outside `{MARKER}` markers')
    return 0


if __name__ == '__main__':
    sys.exit(main())

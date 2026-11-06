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
import sys
import tarfile
import tempfile
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MARKER = 'bashkit fork:'
# Lines above a changed region that may hold its marker comment.
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


def unmarked_hunks(upstream: list[str], fork: list[str]) -> list[int]:
    """1-based fork line numbers of changed regions with no marker nearby."""
    bad = []
    matcher = difflib.SequenceMatcher(a=upstream, b=fork, autojunk=False)
    for tag, _i1, _i2, j1, j2 in matcher.get_opcodes():
        if tag == 'equal':
            continue
        window = fork[max(0, j1 - MARKER_WINDOW) : j2]
        if not any(MARKER in line for line in window):
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
                if dep_spec(name, fork_spec) != dep_spec(name, up_spec):
                    errors.append(f'{where}: [{table}] `{name}` differs from upstream {up_spec!r}')
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

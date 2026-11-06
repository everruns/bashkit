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
# Fork-only packaging files, not upstream source.
FORK_ONLY = {'Cargo.toml', 'LICENSE', 'rustfmt.toml'}


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


def check_crate(fork_dir: Path, upstream_dir: Path) -> list[str]:
    errors = []
    for path in sorted(fork_dir.rglob('*')):
        if not path.is_file():
            continue
        rel = path.relative_to(fork_dir)
        if rel.as_posix() in FORK_ONLY:
            continue
        up = upstream_dir / rel
        fork_lines = path.read_text().splitlines()
        if not up.is_file():
            if not any(MARKER in line for line in fork_lines[:MARKER_WINDOW]):
                errors.append(f'{fork_dir.name}/{rel}: not in upstream and has no `{MARKER}` header')
            continue
        for line in unmarked_hunks(up.read_text().splitlines(), fork_lines):
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
        print(f'{len(errors)} unmarked fork change(s); mark each with `// {MARKER} <why>`', file=sys.stderr)
        return 1
    print(f'Monty forks match upstream {VERSION} outside `{MARKER}` markers')
    return 0


if __name__ == '__main__':
    sys.exit(main())

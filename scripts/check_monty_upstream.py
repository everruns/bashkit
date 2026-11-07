#!/usr/bin/env python3
"""Watch upstream Monty for new releases and advisories against our fork base.

crates/bashkit-monty and crates/bashkit-monty-types are renamed forks of
Monty (see scripts/check_monty_fork.py). Advisory tooling (cargo audit,
cargo deny, Dependabot) matches by crate name, so an advisory filed against
`monty` never reaches the renamed fork crates. This script closes that gap:

- release: fails when crates.io has a newer stable `monty` or `monty-types`
  than the fork base `VERSION` (a re-sync is due, upstream fixes may be
  missing from the fork).
- advisory: fails when OSV (RustSec + GitHub advisories, both ecosystems)
  lists a vulnerability affecting the fork base: crates.io `monty`,
  `monty-types`, and PyPI `pydantic-monty` (same Rust core, advisories are
  often filed there first).

Decision: OSV is queried with the exact base version, so OSV does the
range matching; no local semver range parser. The fork base version comes
from check_monty_fork.VERSION so a re-sync bumps both checks at once.

Decision: an advisory fails the watch even after a release check would pass,
until it is triaged: list it in ACKNOWLEDGED with a reason (fixed in the
fork, or not reachable from bashkit) to silence it.

Usage:
  scripts/check_monty_upstream.py             # both checks
  scripts/check_monty_upstream.py --no-release   # advisories only
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from check_monty_fork import UPSTREAM, VERSION  # noqa: E402

USER_AGENT = 'bashkit-monty-upstream-watch (https://github.com/everruns/bashkit)'
CRATES = tuple(name for name, _sha in UPSTREAM.values())
# (OSV ecosystem, package) pairs whose advisories apply to the fork base.
ADVISORY_PACKAGES = tuple(('crates.io', name) for name in CRATES) + (('PyPI', 'pydantic-monty'),)
# Advisory id -> why it does not affect bashkit (fixed in fork, unreachable).
ACKNOWLEDGED: dict[str, str] = {}


def parse_version(text: str) -> tuple[int, ...] | None:
    """Stable `X.Y.Z` -> tuple; pre-releases and junk -> None."""
    parts = text.split('.')
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        return None
    return tuple(int(p) for p in parts)


def newer_release(base: str, crate_json: dict) -> str | None:
    """Newest stable crates.io version if it is newer than `base`."""
    latest = crate_json.get('crate', {}).get('max_stable_version') or ''
    latest_v, base_v = parse_version(latest), parse_version(base)
    if latest_v is None or base_v is None:
        raise ValueError(f'unparseable version: latest={latest!r} base={base!r}')
    return latest if latest_v > base_v else None


def open_advisories(osv_pages: list[dict], acknowledged: dict[str, str]) -> list[dict]:
    """Unacknowledged vulns from OSV query pages, as {id, summary, aliases}."""
    found = []
    for page in osv_pages:
        for vuln in page.get('vulns') or []:
            # OSV excludes withdrawn records from /v1/query; skip them anyway.
            if not isinstance(vuln, dict) or vuln.get('withdrawn'):
                continue
            # A record without an id still fails the watch, readably.
            vid = str(vuln.get('id') or 'unknown-id')
            aliases = sorted(str(a) for a in vuln.get('aliases') or [])
            if {vid, *aliases} & acknowledged.keys():
                continue
            found.append({'id': vid, 'summary': str(vuln.get('summary') or '').strip(), 'aliases': aliases})
    return found


def _request(url: str, payload: dict | None = None) -> dict:
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(url, data=data, headers={'User-Agent': USER_AGENT})
    if data is not None:
        req.add_header('Content-Type', 'application/json')
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.load(resp)


def fetch_crate(name: str) -> dict:
    return _request(f'https://crates.io/api/v1/crates/{name}')


def fetch_osv(ecosystem: str, name: str, version: str) -> list[dict]:
    pages, token = [], None
    while True:
        query = {'package': {'name': name, 'ecosystem': ecosystem}, 'version': version}
        if token:
            query['page_token'] = token
        page = _request('https://api.osv.dev/v1/query', query)
        pages.append(page)
        token = page.get('next_page_token')
        if not token:
            return pages


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--no-release', action='store_true', help='skip the new-release check')
    args = parser.parse_args()

    errors = []
    if not args.no_release:
        for crate in CRATES:
            latest = newer_release(VERSION, fetch_crate(crate))
            if latest:
                errors.append(
                    f'release: {crate} {latest} is out, fork base is {VERSION}; '
                    're-sync the fork (knowledge/operations/dependencies.md)'
                )
            else:
                print(f'ok: {crate} {VERSION} is the newest stable release')

    for ecosystem, name in ADVISORY_PACKAGES:
        vulns = open_advisories(fetch_osv(ecosystem, name, VERSION), ACKNOWLEDGED)
        for v in vulns:
            aliases = f" ({', '.join(v['aliases'])})" if v['aliases'] else ''
            errors.append(f"advisory: {ecosystem}/{name} {VERSION}: {v['id']}{aliases} {v['summary']}")
        if not vulns:
            print(f'ok: no open advisories for {ecosystem}/{name} {VERSION}')

    prefix = '::error::' if os.environ.get('GITHUB_ACTIONS') else 'error: '
    for e in errors:
        print(f'{prefix}{e}')
    return 1 if errors else 0


if __name__ == '__main__':
    sys.exit(main())

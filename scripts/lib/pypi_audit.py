#!/usr/bin/env python3
"""Advisory scan for one uv lockfile against the OSV database.

Why OSV directly rather than a packaged auditor: `uv` has no audit subcommand,
and pip-audit would be a third tool to install and pin in CI for one lockfile.
OSV is the database GitHub's own PyPI advisories are published into, and
querying it needs nothing beyond the standard library, so the scan costs one
HTTP request and no toolchain.

Only `registry` packages are queried. A uv lockfile also pins the project
itself and its path dependencies (`editable`, `directory`), which no advisory
database has an entry for; one of them has no version at all.

Fails closed. A network or HTTP error is reported as a failed scan rather than
a clean one -- an OSV outage must never read as "no advisories".
"""

import argparse
import json
import sys
import tomllib
import urllib.error
import urllib.request

OSV_BATCH_URL = 'https://api.osv.dev/v1/querybatch'
# OSV accepts up to 1000 queries per batch; stay well under so a lockfile that
# grows does not start failing on a limit.
CHUNK = 250
ATTEMPTS = 3


def registry_packages(lock_path):
    """(name, version) for every package resolved from a package registry."""
    with open(lock_path, 'rb') as handle:
        lock = tomllib.load(handle)

    packages = []
    for package in lock.get('package', []):
        if 'registry' not in (package.get('source') or {}):
            continue
        name, version = package.get('name'), package.get('version')
        if name and version:
            packages.append((name, version))
    return packages


def query_osv(packages, opener=urllib.request.urlopen):
    """Vulnerability ids per package, in the order given.

    Raises RuntimeError if OSV cannot be reached, so the caller fails closed.
    """
    results = []
    for start in range(0, len(packages), CHUNK):
        chunk = packages[start : start + CHUNK]
        payload = {
            'queries': [
                {'package': {'name': name, 'ecosystem': 'PyPI'}, 'version': version}
                for name, version in chunk
            ]
        }
        request = urllib.request.Request(
            OSV_BATCH_URL,
            data=json.dumps(payload).encode(),
            headers={'Content-Type': 'application/json'},
        )

        last_error = None
        for _ in range(ATTEMPTS):
            try:
                with opener(request, timeout=120) as response:
                    body = json.load(response)
                break
            except (urllib.error.URLError, OSError, ValueError) as exc:
                last_error = exc
        else:
            raise RuntimeError(f'OSV query failed: {last_error}')

        batch = body.get('results') or []
        if len(batch) != len(chunk):
            raise RuntimeError(
                f'OSV returned {len(batch)} results for {len(chunk)} queries'
            )
        results.extend([v.get('id') for v in (r.get('vulns') or [])] for r in batch)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('lockfile', help='path to a uv.lock')
    parser.add_argument(
        '--ignore',
        action='append',
        default=[],
        metavar='ID',
        help='OSV or GHSA id to suppress (repeatable)',
    )
    args = parser.parse_args()
    ignored = set(args.ignore)

    try:
        packages = registry_packages(args.lockfile)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        # An unreadable lockfile is a scan that did not run, reported as such
        # rather than as a traceback.
        print(f'error: {args.lockfile}: {exc}', file=sys.stderr)
        return 1

    if not packages:
        print(f'error: no registry packages in {args.lockfile}', file=sys.stderr)
        return 1

    try:
        per_package = query_osv(packages)
    except RuntimeError as exc:
        print(f'error: {args.lockfile}: {exc}', file=sys.stderr)
        return 1

    findings, suppressed = [], []
    for (name, version), vuln_ids in zip(packages, per_package):
        reportable = [i for i in vuln_ids if i not in ignored]
        if reportable:
            findings.append((name, version, reportable))
        suppressed.extend(i for i in vuln_ids if i in ignored)

    if suppressed:
        print(f'{args.lockfile}: {len(suppressed)} suppressed ({", ".join(sorted(suppressed))})')

    if not findings:
        print(
            f'{args.lockfile}: no known vulnerabilities found '
            f'({len(packages)} packages)'
        )
        return 0

    print(f'{args.lockfile}: {len(findings)} vulnerable packages')
    for name, version, vuln_ids in findings:
        print(f'  {name} {version}\t{", ".join(vuln_ids)}')
        for vuln_id in vuln_ids:
            print(f'    https://osv.dev/vulnerability/{vuln_id}')
    return 1


if __name__ == '__main__':
    sys.exit(main())

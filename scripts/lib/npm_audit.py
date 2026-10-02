#!/usr/bin/env python3
"""Advisory scan for one pnpm project, with the central suppressions applied.

Why this wrapper exists rather than a bare `pnpm audit`:

`pnpm audit` has no CLI suppression flag. It reads
`pnpm.auditConfig.ignoreGhsas` out of each project's own package.json, so
suppressing one advisory would mean editing up to five package.json files and
keeping them in agreement with the cargo list in scripts/audit-lockfiles.sh.
The suppression list is a security contract (every entry needs a rationale and
a removal condition in knowledge/security/threat-model.md); it has to live in
one place. The caller passes `--ignore`, and the filtering happens here.

The scan reads the lockfile only: `pnpm audit` resolves the dependency tree
from pnpm-lock.yaml, so neither CI nor a local run needs a `pnpm install`
first.

Fails closed. Output that cannot be parsed means the audit did not run, which
is reported as a failure rather than a clean scan -- a registry outage must
never read as "no advisories".
"""

import argparse
import json
import subprocess
import sys


def parse_report(stdout):
    """Extract the audit JSON from `pnpm audit --json` output.

    pnpm can precede the JSON document with human-readable warnings, so the
    document is located rather than assumed to start at byte zero.
    """
    start = stdout.find('{')
    if start < 0:
        raise ValueError('no JSON object in pnpm audit output')
    report = json.loads(stdout[start:])

    # pnpm reports its own failures as a well-formed JSON document with an
    # `error` key, not as unparseable output. A broken or outdated lockfile
    # therefore arrives looking exactly like a report with no advisories in
    # it, which is the silent pass this whole scan exists to prevent.
    error = report.get('error')
    if error:
        message = (error.get('message') or '').splitlines()
        raise ValueError(
            f"{error.get('code', 'pnpm error')}: {message[0] if message else '?'}"
        )

    # `metadata.vulnerabilities` is the positive signal that an audit actually
    # ran. Requiring it means a document we do not recognise fails closed
    # rather than being read as "nothing found".
    if 'vulnerabilities' not in (report.get('metadata') or {}):
        raise ValueError('no metadata.vulnerabilities in pnpm audit report')

    return report


def advisory_ids(advisory):
    """Every identifier an advisory can be suppressed by (GHSA plus any CVE)."""
    ids = set()
    ghsa = advisory.get('github_advisory_id')
    if ghsa:
        ids.add(ghsa)
    ids.update(advisory.get('cves') or [])
    return ids


def partition(report, ignored):
    """Split the report's advisories into (reportable, suppressed)."""
    reportable, suppressed = [], []
    for advisory in (report.get('advisories') or {}).values():
        matched = advisory_ids(advisory) & ignored
        (suppressed if matched else reportable).append(advisory)
    return reportable, suppressed


def describe(advisory):
    return (
        f"  {advisory.get('severity', '?')}\t"
        f"{advisory.get('module_name', '?')} "
        f"{advisory.get('vulnerable_versions', '?')}\t"
        f"{advisory.get('github_advisory_id', '?')}\n"
        f"    {advisory.get('title', '')}\n"
        f"    {advisory.get('url', '')}"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('project', help='directory holding pnpm-lock.yaml')
    parser.add_argument(
        '--ignore',
        action='append',
        default=[],
        metavar='ID',
        help='GHSA or CVE id to suppress (repeatable)',
    )
    args = parser.parse_args()

    try:
        result = subprocess.run(
            ['pnpm', 'audit', '--json'],
            cwd=args.project,
            capture_output=True,
            text=True,
            timeout=600,
        )
    except FileNotFoundError:
        print('error: pnpm not found on PATH', file=sys.stderr)
        return 1
    except subprocess.TimeoutExpired:
        print(f'error: pnpm audit timed out in {args.project}', file=sys.stderr)
        return 1

    # A non-zero exit is how pnpm reports "advisories found", so it is not an
    # error on its own; unparseable output is.
    try:
        report = parse_report(result.stdout)
    except ValueError as exc:
        print(f'error: pnpm audit failed in {args.project}: {exc}', file=sys.stderr)
        if result.stderr.strip():
            print(result.stderr.strip(), file=sys.stderr)
        return 1

    reportable, suppressed = partition(report, set(args.ignore))

    if suppressed:
        # Report the ids that matched the list, not every id the advisory
        # carries, so the count and the names agree.
        matched = sorted(i for a in suppressed for i in advisory_ids(a) & set(args.ignore))
        print(f'{args.project}: {len(suppressed)} suppressed ({", ".join(matched)})')

    if not reportable:
        print(f'{args.project}: no known vulnerabilities found')
        return 0

    print(f'{args.project}: {len(reportable)} advisories')
    for advisory in sorted(
        reportable, key=lambda a: (a.get('module_name', ''), a.get('github_advisory_id', ''))
    ):
        print(describe(advisory))
    return 1


if __name__ == '__main__':
    sys.exit(main())

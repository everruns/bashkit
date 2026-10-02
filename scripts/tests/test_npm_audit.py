"""Unit tests for the npm half of the advisory scan.

Offline by construction: `pnpm audit` is never invoked, the recorded report
shapes are. The fixtures below are trimmed from a real
`pnpm audit --json` run against the pre-#2488 `crates/bashkit-js` lockfile,
which carried three live `brace-expansion` advisories.

What matters here is that a clean report cannot be confused with a failed one.
The scan is the only thing standing between a published npm advisory and a
silent lockfile, so "no advisories" has to mean the audit ran and found
nothing.
"""

import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


def load_module():
    spec = importlib.util.spec_from_file_location(
        'npm_audit', ROOT / 'scripts/lib/npm_audit.py'
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


npm_audit = load_module()

ADVISORY = {
    'id': 1240105,
    'github_advisory_id': 'GHSA-qhr7-859c-m2p7',
    'cves': ['CVE-2026-102278'],
    'severity': 'high',
    'module_name': 'brace-expansion',
    'vulnerable_versions': '>=2.0.0 <2.1.6',
    'title': 'brace-expansion: DoS via uncontrolled recursion',
    'url': 'https://github.com/advisories/GHSA-qhr7-859c-m2p7',
    'findings': [{'version': '2.1.4', 'paths': ['.>ava>@vercel/nft>glob>minimatch>brace-expansion']}],
}

CLEAN_REPORT = {
    'actions': [],
    'advisories': {},
    'muted': [],
    'metadata': {'vulnerabilities': {'info': 0, 'low': 0, 'moderate': 0, 'high': 0, 'critical': 0}},
}


class ParseReportTests(unittest.TestCase):
    def test_parses_a_clean_report(self):
        report = npm_audit.parse_report(json.dumps(CLEAN_REPORT))
        self.assertEqual(report['advisories'], {})

    def test_skips_warnings_printed_before_the_json(self):
        # pnpm prefixes human-readable warnings often enough that assuming the
        # document starts at byte zero is a real failure mode.
        report = npm_audit.parse_report(f' WARN  something\n{json.dumps(CLEAN_REPORT)}\n')
        self.assertEqual(report['advisories'], {})

    def test_output_without_json_is_an_error(self):
        # Fail closed: a registry outage prints no document, and that must not
        # read as a clean scan.
        for stdout in ('', 'ERR_PNPM_AUDIT_ENDPOINT_NOT_EXISTS  no audit endpoint'):
            with self.subTest(stdout=stdout):
                with self.assertRaises(ValueError):
                    npm_audit.parse_report(stdout)

    def test_a_pnpm_error_document_is_not_a_clean_report(self):
        # The regression that motivated the check: pnpm reports a broken or
        # outdated lockfile as well-formed JSON with an `error` key, which
        # parses cleanly and carries no advisories. Read naively it is
        # indistinguishable from "nothing found" -- a lockfile the scan cannot
        # read would have passed.
        broken = json.dumps(
            {
                'error': {
                    'code': 'ERR_PNPM_BROKEN_LOCKFILE',
                    'message': 'The lockfile at "/x/pnpm-lock.yaml" is broken:\n 1 | bogus',
                }
            }
        )
        with self.assertRaises(ValueError) as caught:
            npm_audit.parse_report(broken)
        self.assertIn('ERR_PNPM_BROKEN_LOCKFILE', str(caught.exception))

    def test_an_outdated_lockfile_error_is_not_a_clean_report(self):
        outdated = json.dumps(
            {'error': {'code': 'ERR_PNPM_OUTDATED_LOCKFILE', 'message': 'out of date'}}
        )
        with self.assertRaises(ValueError):
            npm_audit.parse_report(outdated)

    def test_a_document_without_audit_metadata_is_an_error(self):
        # `metadata.vulnerabilities` is the positive signal that an audit ran;
        # anything lacking it is some other document, not a clean scan.
        for document in ('{}', '{"advisories": {}}', '{"metadata": {}}'):
            with self.subTest(document=document):
                with self.assertRaises(ValueError):
                    npm_audit.parse_report(document)


class PartitionTests(unittest.TestCase):
    def test_reports_an_unsuppressed_advisory(self):
        reportable, suppressed = npm_audit.partition({'advisories': {'1': ADVISORY}}, set())
        self.assertEqual(len(reportable), 1)
        self.assertEqual(suppressed, [])

    def test_suppresses_by_ghsa_id(self):
        reportable, suppressed = npm_audit.partition(
            {'advisories': {'1': ADVISORY}}, {'GHSA-qhr7-859c-m2p7'}
        )
        self.assertEqual(reportable, [])
        self.assertEqual(len(suppressed), 1)

    def test_suppresses_by_cve_id(self):
        # The advisory list in audit-lockfiles.sh is written by whichever id the
        # advisory is known by, so both have to resolve.
        reportable, suppressed = npm_audit.partition(
            {'advisories': {'1': ADVISORY}}, {'CVE-2026-102278'}
        )
        self.assertEqual(reportable, [])
        self.assertEqual(len(suppressed), 1)

    def test_an_unrelated_suppression_does_not_hide_an_advisory(self):
        reportable, _ = npm_audit.partition(
            {'advisories': {'1': ADVISORY}}, {'GHSA-0000-0000-0000'}
        )
        self.assertEqual(len(reportable), 1)

    def test_clean_report_has_nothing_to_report(self):
        reportable, suppressed = npm_audit.partition(CLEAN_REPORT, set())
        self.assertEqual((reportable, suppressed), ([], []))

    def test_missing_advisories_key_is_treated_as_clean(self):
        reportable, suppressed = npm_audit.partition({'metadata': {}}, set())
        self.assertEqual((reportable, suppressed), ([], []))


class AdvisoryIdsTests(unittest.TestCase):
    def test_collects_ghsa_and_cves(self):
        self.assertEqual(
            npm_audit.advisory_ids(ADVISORY),
            {'GHSA-qhr7-859c-m2p7', 'CVE-2026-102278'},
        )

    def test_tolerates_an_advisory_with_neither(self):
        self.assertEqual(npm_audit.advisory_ids({}), set())


class DescribeTests(unittest.TestCase):
    def test_names_the_package_and_the_advisory(self):
        text = npm_audit.describe(ADVISORY)
        self.assertIn('brace-expansion', text)
        self.assertIn('GHSA-qhr7-859c-m2p7', text)
        self.assertIn('high', text)

    def test_tolerates_a_sparse_advisory(self):
        # A shape change upstream should degrade the report, not crash the scan.
        self.assertIn('?', npm_audit.describe({}))


if __name__ == '__main__':
    unittest.main()

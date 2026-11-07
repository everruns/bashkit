"""Tests for scripts/check_monty_upstream.py (offline, canned API responses)."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_monty_upstream as cmu  # noqa: E402


def crate(max_stable):
    return {'crate': {'name': 'monty', 'max_stable_version': max_stable, 'newest_version': max_stable}}


class VersionTests(unittest.TestCase):
    def test_parse_stable(self):
        self.assertEqual(cmu.parse_version('1.4.0'), (1, 4, 0))

    def test_parse_rejects_prerelease_and_junk(self):
        for bad in ('1.5.0-rc.1', '1.5', '', 'v1.4.0', '1.4.x'):
            self.assertIsNone(cmu.parse_version(bad), bad)

    def test_newer_release_found(self):
        self.assertEqual(cmu.newer_release('1.4.0', crate('1.5.0')), '1.5.0')

    def test_numeric_not_lexical_compare(self):
        self.assertEqual(cmu.newer_release('1.9.0', crate('1.10.0')), '1.10.0')

    def test_same_or_older_is_none(self):
        self.assertIsNone(cmu.newer_release('1.4.0', crate('1.4.0')))
        self.assertIsNone(cmu.newer_release('1.4.0', crate('1.3.9')))

    def test_missing_version_fails_loudly(self):
        # A changed API shape must not read as "no new release".
        with self.assertRaises(ValueError):
            cmu.newer_release('1.4.0', {'crate': {}})
        with self.assertRaises(ValueError):
            cmu.newer_release('1.4.0', {})


class AdvisoryTests(unittest.TestCase):
    VULN = {'id': 'GHSA-xxxx-yyyy-zzzz', 'aliases': ['RUSTSEC-2026-0001'], 'summary': ' sandbox escape '}

    def test_empty_osv_response(self):
        self.assertEqual(cmu.open_advisories([{}], {}), [])

    def test_vuln_reported(self):
        found = cmu.open_advisories([{'vulns': [self.VULN]}], {})
        self.assertEqual(found, [{'id': 'GHSA-xxxx-yyyy-zzzz', 'summary': 'sandbox escape',
                                  'aliases': ['RUSTSEC-2026-0001']}])

    def test_acknowledged_by_id_or_alias(self):
        self.assertEqual(cmu.open_advisories([{'vulns': [self.VULN]}], {'GHSA-xxxx-yyyy-zzzz': 'fixed'}), [])
        self.assertEqual(cmu.open_advisories([{'vulns': [self.VULN]}], {'RUSTSEC-2026-0001': 'fixed'}), [])

    def test_all_pages_scanned(self):
        other = {'id': 'GHSA-2', 'summary': 'x'}
        found = cmu.open_advisories([{'vulns': [self.VULN], 'next_page_token': 't'}, {'vulns': [other]}], {})
        self.assertEqual([v['id'] for v in found], ['GHSA-xxxx-yyyy-zzzz', 'GHSA-2'])


    def test_malformed_records_stay_readable(self):
        page = {'vulns': [{'aliases': None, 'summary': None}, {'id': 'GHSA-3', 'withdrawn': '2026-01-01T00:00:00Z'}]}
        self.assertEqual(cmu.open_advisories([page], {}), [{'id': 'unknown-id', 'summary': '', 'aliases': []}])
        self.assertEqual(cmu.open_advisories([{'vulns': None}], {}), [])


class ConfigTests(unittest.TestCase):
    def test_watches_both_crates_and_pypi(self):
        self.assertEqual(set(cmu.CRATES), {'monty', 'monty-types'})
        self.assertIn(('PyPI', 'pydantic-monty'), cmu.ADVISORY_PACKAGES)
        self.assertIn(('crates.io', 'monty'), cmu.ADVISORY_PACKAGES)

    def test_acknowledged_entries_have_reasons(self):
        for vid, why in cmu.ACKNOWLEDGED.items():
            self.assertTrue(why.strip(), vid)


if __name__ == '__main__':
    unittest.main()

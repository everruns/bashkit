"""Unit tests for the PyPI half of the advisory scan.

Offline by construction: OSV is never contacted, a fake opener stands in for
it. The uv lockfile shapes below are the ones `examples/docs-grep-agent/uv.lock`
actually holds -- 47 registry packages plus a `directory` dependency with no
version and an `editable` entry for the project itself.

The fail-closed cases are the point. An OSV outage, or a batch that comes back
the wrong size, must be reported as a failed scan; if either one could return
"no vulnerabilities", the scan would be at its most reassuring exactly when it
had stopped working.
"""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
import unittest.mock
import urllib.error

ROOT = Path(__file__).resolve().parents[2]


def load_module():
    spec = importlib.util.spec_from_file_location(
        'pypi_audit', ROOT / 'scripts/lib/pypi_audit.py'
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


pypi_audit = load_module()

LOCK = """
version = 1

[[package]]
name = "anyio"
version = "4.14.2"
source = { registry = "https://pypi.org/simple" }

[[package]]
name = "urllib3"
version = "2.7.0"
source = { registry = "https://pypi.org/simple" }

[[package]]
name = "bashkit"
source = { directory = "../../crates/bashkit-python" }

[[package]]
name = "bashkit-docs-grep-agent"
version = "0.1.0"
source = { editable = "." }
"""


def write_lock(text=LOCK):
    handle = tempfile.NamedTemporaryFile('w', suffix='.lock', delete=False)
    handle.write(text)
    handle.close()
    return handle.name


def fake_opener(results):
    """Stand-in for urlopen that replays `results` as one OSV batch response."""

    def opener(request, timeout=None):
        body = json.dumps({'results': results}).encode()
        return io.BytesIO(body)

    return opener


class RegistryPackagesTests(unittest.TestCase):
    def test_keeps_only_registry_packages(self):
        # A path or editable dependency has no advisory-database entry, and one
        # of them carries no version at all.
        packages = pypi_audit.registry_packages(write_lock())
        self.assertEqual(packages, [('anyio', '4.14.2'), ('urllib3', '2.7.0')])

    def test_a_lockfile_with_no_packages_yields_nothing(self):
        self.assertEqual(pypi_audit.registry_packages(write_lock('version = 1\n')), [])


class QueryOsvTests(unittest.TestCase):
    def test_returns_vulnerability_ids_per_package(self):
        results = [{}, {'vulns': [{'id': 'GHSA-pq67-6m6q-mj2v'}, {'id': 'PYSEC-2026-1'}]}]
        found = pypi_audit.query_osv(
            [('anyio', '4.14.2'), ('urllib3', '2.7.0')], opener=fake_opener(results)
        )
        self.assertEqual(found, [[], ['GHSA-pq67-6m6q-mj2v', 'PYSEC-2026-1']])

    def test_a_network_failure_is_an_error_not_a_clean_scan(self):
        def failing(request, timeout=None):
            raise urllib.error.URLError('osv unreachable')

        with self.assertRaises(RuntimeError):
            pypi_audit.query_osv([('anyio', '4.14.2')], opener=failing)

    def test_a_short_batch_response_is_an_error(self):
        # Silently zipping a short response would drop the tail of the
        # lockfile from the scan.
        with self.assertRaises(RuntimeError):
            pypi_audit.query_osv(
                [('anyio', '4.14.2'), ('urllib3', '2.7.0')],
                opener=fake_opener([{}]),
            )

    def test_malformed_json_is_an_error(self):
        def garbage(request, timeout=None):
            return io.BytesIO(b'<html>502 Bad Gateway</html>')

        with self.assertRaises(RuntimeError):
            pypi_audit.query_osv([('anyio', '4.14.2')], opener=garbage)

    def test_batches_larger_than_the_chunk_size(self):
        # The lockfile is well under the chunk size today; this pins the
        # batching so growth past it does not start dropping packages.
        packages = [(f'pkg{i}', '1.0.0') for i in range(pypi_audit.CHUNK + 5)]
        sizes = []

        def counting(request, timeout=None):
            count = len(json.loads(request.data)['queries'])
            sizes.append(count)
            return io.BytesIO(json.dumps({'results': [{}] * count}).encode())

        found = pypi_audit.query_osv(packages, opener=counting)
        self.assertEqual(len(found), len(packages))
        self.assertEqual(sizes, [pypi_audit.CHUNK, 5])


class MainTests(unittest.TestCase):
    """End-to-end exit codes, with OSV replaced by a canned response."""

    def run_main(self, argv, per_package):
        original = pypi_audit.query_osv
        pypi_audit.query_osv = lambda packages, **kwargs: per_package
        stdout = io.StringIO()
        try:
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
                with unittest.mock.patch.object(sys, 'argv', ['pypi_audit.py', *argv]):
                    code = pypi_audit.main()
        finally:
            pypi_audit.query_osv = original
        return code, stdout.getvalue()

    def test_a_clean_lockfile_passes(self):
        code, output = self.run_main([write_lock()], [[], []])
        self.assertEqual(code, 0)
        self.assertIn('no known vulnerabilities found', output)

    def test_a_vulnerable_package_fails_and_is_named(self):
        code, output = self.run_main([write_lock()], [[], ['GHSA-pq67-6m6q-mj2v']])
        self.assertEqual(code, 1)
        self.assertIn('urllib3 2.7.0', output)
        self.assertIn('GHSA-pq67-6m6q-mj2v', output)

    def test_a_suppressed_id_passes_and_is_still_reported(self):
        # Suppressed is not invisible: the scan still says what it skipped, so
        # a stale suppression is noticeable rather than silently permanent.
        code, output = self.run_main(
            [write_lock(), '--ignore', 'GHSA-pq67-6m6q-mj2v'],
            [[], ['GHSA-pq67-6m6q-mj2v']],
        )
        self.assertEqual(code, 0)
        self.assertIn('1 suppressed', output)

    def test_an_unrelated_suppression_does_not_hide_a_vulnerability(self):
        code, _ = self.run_main(
            [write_lock(), '--ignore', 'GHSA-0000-0000-0000'],
            [[], ['GHSA-pq67-6m6q-mj2v']],
        )
        self.assertEqual(code, 1)

    def test_a_lockfile_with_no_registry_packages_is_an_error(self):
        # Nothing to audit means the scan did not run, not that it was clean.
        code, _ = self.run_main([write_lock('version = 1\n')], [])
        self.assertEqual(code, 1)

    def test_an_unreadable_lockfile_is_an_error_not_a_traceback(self):
        # A malformed lockfile must be reported as a failed scan; a traceback
        # is a non-zero exit by accident rather than by contract.
        code, _ = self.run_main([write_lock('version = 1\nbogus = [\n')], [])
        self.assertEqual(code, 1)

    def test_a_missing_lockfile_is_an_error(self):
        code, _ = self.run_main(['/nonexistent/uv.lock'], [])
        self.assertEqual(code, 1)


if __name__ == '__main__':
    unittest.main()

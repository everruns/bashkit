"""Keep the advisory scan covering every lockfile, in every ecosystem, from every caller.

Three regressions this pins down. Listing lockfiles by hand missed whole
workspaces twice (see the 2026-08-22 entry in knowledge/log.md), so discovery
has to stay discovery. The scan has two callers — CI on push, nightly on a
schedule — so the suppression lists must not get re-inlined into one of them
where they can drift away from deny.toml. And the scan was cargo-only until
2026-10-02, which left seven advisories sitting in the npm and PyPI lockfiles
with no job that would ever fail, so each ecosystem now asserts its own floor.
"""

import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import tomllib
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/audit-lockfiles.sh'

# Stubs that record each invocation and report success. Each exits non-zero for
# a path under a directory named "poison", which is how the failure-propagation
# cases are triggered. The label distinguishes which ecosystem called.
STUB = """#!/bin/sh
echo "{label} $@" >> "$AUDIT_CALLS"
case "$*" in
  *poison*) exit 1 ;;
esac
exit 0
"""

ALL_LOCKS = [
    'Cargo.lock',
    'crates/bashkit/fuzz/Cargo.lock',
    'crates/bashkit-js/pnpm-lock.yaml',
    'site/pnpm-lock.yaml',
    'examples/docs-grep-agent/uv.lock',
]


class AuditLockfilesTests(unittest.TestCase):
    def run_script(self, locks):
        """Run the script against a temp tree containing `locks`, return (result, calls)."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'scripts/lib').mkdir(parents=True)
            shutil.copy(SCRIPT, root / 'scripts/audit-lockfiles.sh')
            (root / 'scripts/audit-lockfiles.sh').chmod(0o755)

            for lock in locks:
                path = root / lock
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('# lockfile\n')

            bin_dir = root / 'stub-bin'
            bin_dir.mkdir()
            # cargo is resolved through PATH; the ecosystem helpers are
            # resolved out of the script's own tree, so they are stubbed there.
            for target, label in (
                (bin_dir / 'cargo', 'cargo'),
                (root / 'scripts/lib/npm_audit.py', 'npm'),
                (root / 'scripts/lib/pypi_audit.py', 'pypi'),
            ):
                target.write_text(STUB.format(label=label))
                target.chmod(target.stat().st_mode | stat.S_IEXEC)

            calls_file = root / 'calls.txt'
            env = {
                **os.environ,
                'AUDIT_CALLS': str(calls_file),
                'PATH': str(bin_dir) + os.pathsep + os.environ['PATH'],
            }
            result = subprocess.run(
                [str(root / 'scripts/audit-lockfiles.sh')],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
            )
            calls = calls_file.read_text().splitlines() if calls_file.exists() else []
            return result, calls

    @staticmethod
    def by_label(calls, label):
        return [c for c in calls if c.startswith(f'{label} ')]

    def test_audits_every_discovered_lockfile(self):
        result, calls = self.run_script(
            ALL_LOCKS + ['examples/hyperlight/Cargo.lock', 'examples/pnpm-lock.yaml']
        )
        self.assertEqual(result.returncode, 0, result.stderr)

        cargo = {c.split('-f ')[1] for c in self.by_label(calls, 'cargo')}
        self.assertEqual(
            cargo,
            {
                './Cargo.lock',
                './crates/bashkit/fuzz/Cargo.lock',
                './examples/hyperlight/Cargo.lock',
            },
        )
        # The npm helper audits a project directory, not the lockfile path:
        # `pnpm audit` runs inside the project.
        npm = {c.split()[-1] for c in self.by_label(calls, 'npm')}
        self.assertEqual(
            npm, {'./crates/bashkit-js', './site', './examples'}
        )
        pypi = {c.split()[-1] for c in self.by_label(calls, 'pypi')}
        self.assertEqual(pypi, {'./examples/docs-grep-agent/uv.lock'})

    def test_passes_suppressions_to_every_audit(self):
        _, calls = self.run_script(ALL_LOCKS)
        for call in self.by_label(calls, 'cargo'):
            self.assertIn('--ignore RUSTSEC-2023-0071', call)

    def test_skips_build_and_dependency_directories(self):
        result, calls = self.run_script(
            ALL_LOCKS
            + [
                'target/debug/Cargo.lock',
                'crates/bashkit-js/node_modules/somedep/Cargo.lock',
                'site/node_modules/somedep/pnpm-lock.yaml',
                'target/x/uv.lock',
            ]
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.by_label(calls, 'cargo')), 2, calls)
        self.assertEqual(len(self.by_label(calls, 'npm')), 2, calls)
        self.assertEqual(len(self.by_label(calls, 'pypi')), 1, calls)

    def test_one_failing_lockfile_fails_the_scan(self):
        # ...and the remaining lockfiles are still audited, so one advisory
        # does not hide the others.
        result, calls = self.run_script(ALL_LOCKS + ['poison/Cargo.lock'])
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(self.by_label(calls, 'cargo')), 3, calls)

    def test_a_failure_in_one_ecosystem_still_audits_the_others(self):
        # An npm advisory must not stop the cargo or PyPI scan: each ecosystem
        # is reported separately so one never masks another.
        result, calls = self.run_script(ALL_LOCKS + ['poison/pnpm-lock.yaml'])
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(self.by_label(calls, 'npm')), 3, calls)
        self.assertEqual(len(self.by_label(calls, 'cargo')), 2, calls)
        self.assertEqual(len(self.by_label(calls, 'pypi')), 1, calls)

    def test_each_ecosystem_fails_when_it_discovers_nothing(self):
        # A scan that audited nothing is broken, not clean -- and that holds
        # per ecosystem, so a lockfile kind vanishing from discovery fails
        # instead of quietly narrowing the scan.
        for missing, lockname in (
            ('Cargo.lock', 'Cargo.lock'),
            ('pnpm-lock.yaml', 'pnpm-lock.yaml'),
            ('uv.lock', 'uv.lock'),
        ):
            with self.subTest(missing=missing):
                locks = [lock for lock in ALL_LOCKS if not lock.endswith(missing)]
                result, calls = self.run_script(locks)
                self.assertEqual(result.returncode, 1)
                self.assertIn(f'no {lockname} found', result.stderr)
                # The other ecosystems are still audited.
                self.assertTrue(calls)

    def test_finding_no_lockfile_at_all_is_a_failure(self):
        result, calls = self.run_script([])
        self.assertEqual(result.returncode, 1)
        self.assertEqual(calls, [])
        for lockname in ('Cargo.lock', 'pnpm-lock.yaml', 'uv.lock'):
            self.assertIn(f'no {lockname} found', result.stderr)

    def test_suppressions_are_a_subset_of_deny_toml(self):
        # cargo-audit needs only the advisories that actually fail it, but it
        # must never ignore something cargo-deny still enforces.
        script = SCRIPT.read_text()
        in_list = script.split('IGNORED_ADVISORIES=(')[1].split(')')[0]
        script_ignores = {
            line.strip() for line in in_list.splitlines() if line.strip()
        }
        with open(ROOT / 'deny.toml', 'rb') as handle:
            deny_ignores = set(tomllib.load(handle)['advisories']['ignore'])
        self.assertTrue(script_ignores)
        self.assertLessEqual(script_ignores, deny_ignores)

    def test_every_ecosystem_has_a_suppression_list(self):
        # The escape hatch has to exist per ecosystem, otherwise an advisory
        # with no available fix gets "handled" by pinning a floor nobody
        # records -- which is what left the npm lockfiles stale in the first
        # place.
        script = SCRIPT.read_text()
        for name in (
            'IGNORED_ADVISORIES=(',
            'IGNORED_NPM_ADVISORIES=(',
            'IGNORED_PYPI_ADVISORIES=(',
        ):
            self.assertIn(name, script)

    def test_both_workflows_run_the_shared_script(self):
        # The scan has two callers by design: push-triggered CI and the nightly
        # schedule. Re-inlining either one reintroduces a second copy of the
        # suppression lists. Both must also set up pnpm, or the npm half of the
        # scan fails on a missing tool rather than on an advisory.
        for workflow, job in (
            ('.github/workflows/ci.yml', 'audit'),
            ('.github/workflows/nightly.yml', 'advisories'),
        ):
            with self.subTest(workflow=workflow):
                spec = yaml.safe_load((ROOT / workflow).read_text())
                steps = spec['jobs'][job]['steps']
                runs = '\n'.join(step.get('run', '') for step in steps)
                self.assertIn('./scripts/audit-lockfiles.sh', runs)
                self.assertNotIn('cargo audit', runs)
                self.assertNotIn('pnpm audit', runs)
                uses = '\n'.join(step.get('uses', '') for step in steps)
                self.assertIn('pnpm/action-setup', uses)
                self.assertIn('actions/setup-node', uses)


if __name__ == '__main__':
    unittest.main()

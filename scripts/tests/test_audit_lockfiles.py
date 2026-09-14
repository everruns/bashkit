"""Keep the advisory scan covering every lockfile, from every caller.

Two regressions this pins down. Listing lockfiles by hand missed whole
workspaces twice (see the 2026-08-22 entry in knowledge/log.md), so discovery
has to stay discovery. And the scan now has two callers — CI on push, nightly
on a schedule — so the suppression list must not get re-inlined into one of
them where it can drift away from deny.toml.
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

# A `cargo` stub that records each invocation and reports success. Exits
# non-zero for a lockfile under a directory named "poison", which is how the
# failure-propagation case is triggered.
CARGO_STUB = """#!/bin/sh
echo "$@" >> "$AUDIT_CALLS"
case "$*" in
  *poison*) exit 1 ;;
esac
exit 0
"""


class AuditLockfilesTests(unittest.TestCase):
    def run_script(self, locks):
        """Run the script against a temp tree containing `locks`, return (rc, calls)."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'scripts').mkdir()
            shutil.copy(SCRIPT, root / 'scripts/audit-lockfiles.sh')
            (root / 'scripts/audit-lockfiles.sh').chmod(0o755)

            for lock in locks:
                path = root / lock
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('# lockfile\n')

            bin_dir = root / 'stub-bin'
            bin_dir.mkdir()
            cargo = bin_dir / 'cargo'
            cargo.write_text(CARGO_STUB)
            cargo.chmod(cargo.stat().st_mode | stat.S_IEXEC)

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

    def test_audits_every_discovered_lockfile(self):
        result, calls = self.run_script(
            [
                'Cargo.lock',
                'crates/bashkit/fuzz/Cargo.lock',
                'examples/hyperlight/Cargo.lock',
                'examples/hyperlight/host/Cargo.lock',
            ]
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(calls), 4, calls)
        audited = {call.split('-f ')[1] for call in calls}
        self.assertEqual(
            audited,
            {
                './Cargo.lock',
                './crates/bashkit/fuzz/Cargo.lock',
                './examples/hyperlight/Cargo.lock',
                './examples/hyperlight/host/Cargo.lock',
            },
        )

    def test_passes_suppressions_to_every_audit(self):
        _, calls = self.run_script(['Cargo.lock', 'crates/bashkit/fuzz/Cargo.lock'])
        for call in calls:
            self.assertIn('--ignore RUSTSEC-2023-0071', call)

    def test_skips_build_and_dependency_directories(self):
        result, calls = self.run_script(
            [
                'Cargo.lock',
                'target/debug/Cargo.lock',
                'crates/bashkit-js/node_modules/somedep/Cargo.lock',
            ]
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(calls), 1, calls)
        self.assertIn('./Cargo.lock', calls[0])

    def test_one_failing_lockfile_fails_the_scan(self):
        # ...and the remaining lockfiles are still audited, so one advisory
        # does not hide the others.
        result, calls = self.run_script(['Cargo.lock', 'poison/Cargo.lock'])
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(calls), 2, calls)

    def test_finding_no_lockfile_is_a_failure(self):
        # A scan that audited nothing is broken, not clean.
        result, calls = self.run_script([])
        self.assertEqual(result.returncode, 1)
        self.assertEqual(calls, [])
        self.assertIn('no Cargo.lock found', result.stderr)

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

    def test_both_workflows_run_the_shared_script(self):
        # The scan has two callers by design: push-triggered CI and the nightly
        # schedule. Re-inlining either one reintroduces a second copy of the
        # suppression list.
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


if __name__ == '__main__':
    unittest.main()

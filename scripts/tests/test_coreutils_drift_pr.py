"""The coreutils drift run must open a PR when the previous one was closed.

`gh pr view <branch>` resolves a pull request for that branch in ANY state.
Once a drift PR is closed unmerged, it keeps matching forever, so the weekly
run silently edited that closed PR instead of opening a new one and the drift
stopped surfacing. These tests execute the workflow's own `run` script with
stubbed `gh`/`git` to pin the open-PR-only lookup.

They also pin the token split. The Actions job token cannot create pull
requests, so the PR step runs on a Doppler-resolved PAT; with no PAT it must
fail loudly rather than skip the PR and leave the drift invisible.
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / '.github/workflows/coreutils-args-drift.yml'

def _write(path: Path, body: str) -> None:
    path.write_text(body)
    path.chmod(0o755)


class CoreutilsDriftPrTests(unittest.TestCase):
    def setUp(self):
        jobs = yaml.safe_load(WORKFLOW.read_text())['jobs']
        self.step = next(
            step
            for job in jobs.values()
            for step in job.get('steps', [])
            if step.get('name') == 'Open or update pull request'
        )

    def _run(self, *, open_pr_exists: bool, have_pat: bool = True,
             expect_success: bool = True):
        """Run the step with a closed PR present and `open_pr_exists` open ones."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bin_dir = root / 'bin'
            bin_dir.mkdir()
            log = root / 'gh.log'
            open_number = '4242' if open_pr_exists else ''

            # `gh pr view` always succeeds (the closed PR still matches), so a
            # lookup that trusts it cannot tell closed from open.
            _write(bin_dir / 'gh', f'''#!/bin/sh
printf '%s\\n' "$*" >> {log}
case "$1 $2" in
  "auth setup-git") exit 0 ;;
  "pr list") printf '%s' '{open_number}' ; exit 0 ;;
  "pr view") exit 0 ;;
  "pr edit"|"pr create") exit 0 ;;
esac
exit 0
''')
            _write(bin_dir / 'git', '''#!/bin/sh
exit 0
''')

            env = {
                'PATH': f"{bin_dir}:{os.environ['PATH']}",
                'HOME': str(root),
                'GITHUB_REPOSITORY': 'everruns/bashkit',
                'GH_TOKEN': 'stub' if have_pat else '',
                'HAVE_PAT': 'true' if have_pat else 'false',
                'RUNNER_TEMP': str(root),
                'NEW_REV': 'abc1234',
                'OLD_REV': 'def5678',
                'UTILS': 'cat tac',
                'MODULES': 'format',
            }
            result = subprocess.run(
                ['/bin/bash', '-c', self.step['run']],
                env=env, capture_output=True, text=True, cwd=root,
            )
            if expect_success:
                self.assertEqual(result.returncode, 0, result.stderr)
            else:
                self.assertNotEqual(result.returncode, 0, result.stdout)
            calls = log.read_text() if log.exists() else ''
            return calls, result

    def test_opens_a_new_pr_when_only_a_closed_one_matches(self):
        calls, _ = self._run(open_pr_exists=False)
        self.assertIn('pr create', calls, calls)
        self.assertNotIn('pr edit', calls, calls)

    def test_updates_the_existing_pr_when_one_is_open(self):
        calls, _ = self._run(open_pr_exists=True)
        self.assertIn('pr edit', calls, calls)
        self.assertNotIn('pr create', calls, calls)

    def test_open_state_is_scoped_in_the_lookup(self):
        self.assertIn('--state open', self.step['run'])

    def test_missing_pat_fails_loudly_without_touching_the_pr(self):
        """No PAT must be a red run, not a silently skipped PR.

        The job token can push but cannot create a PR. If this step merely
        skipped the PR, the drift would sit on a force-pushed branch that
        nobody looks at and the run would go green.
        """
        calls, result = self._run(
            open_pr_exists=False, have_pat=False, expect_success=False,
        )
        self.assertNotIn('pr create', calls, calls)
        self.assertNotIn('pr edit', calls, calls)
        self.assertIn('::error::', result.stdout, result.stdout)
        # The message has to name the recovery path: the drift is already
        # pushed, so the branch is where a human picks it up.
        self.assertIn('chore/coreutils-args-drift', result.stdout)

    def test_push_and_pr_use_separate_tokens(self):
        """The push runs on the job token; only the PR gets the PAT."""
        jobs = yaml.safe_load(WORKFLOW.read_text())['jobs']
        steps = {
            step.get('name'): step
            for job in jobs.values()
            for step in job.get('steps', [])
        }
        push = steps['Commit and push drift branch']
        self.assertEqual(push['env']['GH_TOKEN'], '${{ github.token }}')
        self.assertIn('git push', push['run'])
        self.assertIn('steps.pat.outputs.token', self.step['env']['GH_TOKEN'])
        self.assertNotIn('git push', self.step['run'])


if __name__ == '__main__':
    unittest.main()

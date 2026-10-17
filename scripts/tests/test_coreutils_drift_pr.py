"""The coreutils drift run must open a PR when the previous one was closed.

`gh pr view <branch>` resolves a pull request for that branch in ANY state.
Once a drift PR is closed unmerged, it keeps matching forever, so the weekly
run silently edited that closed PR instead of opening a new one and the drift
stopped surfacing. These tests execute the workflow's own `run` script with
stubbed `gh`/`git` to pin the open-PR-only lookup.
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
            if step.get('name') == 'Commit and open PR'
        )

    def _run(self, *, open_pr_exists: bool):
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
                'GH_TOKEN': 'stub',
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
            self.assertEqual(result.returncode, 0, result.stderr)
            return log.read_text() if log.exists() else ''

    def test_opens_a_new_pr_when_only_a_closed_one_matches(self):
        calls = self._run(open_pr_exists=False)
        self.assertIn('pr create', calls, calls)
        self.assertNotIn('pr edit', calls, calls)

    def test_updates_the_existing_pr_when_one_is_open(self):
        calls = self._run(open_pr_exists=True)
        self.assertIn('pr edit', calls, calls)
        self.assertNotIn('pr create', calls, calls)

    def test_open_state_is_scoped_in_the_lookup(self):
        self.assertIn('--state open', self.step['run'])


if __name__ == '__main__':
    unittest.main()

"""Keep CI credentials read-only and release examples on reviewed dependencies."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]


class CISupplyChainTests(unittest.TestCase):
    def test_ci_has_no_write_permission_or_persisted_checkout_token(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/ci.yml').read_text())
        self.assertEqual(workflow['permissions'], {'contents': 'read'})
        for job in workflow['jobs'].values():
            self.assertNotIn('write', job.get('permissions', {}).values())
            for step in job.get('steps', []):
                if step.get('uses', '').startswith('actions/checkout@'):
                    self.assertIs(step.get('with', {}).get('persist-credentials'), False)

    def test_release_examples_install_lockfile_and_use_built_binding(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/publish-js.yml').read_text())
        for job_name in ('test-js-macos-windows', 'test-js-linux'):
            job = workflow['jobs'][job_name]
            self.assertNotIn('pnpm add', '\n'.join(step.get('run', '') for step in job['steps']))
            step = next(s for s in job['steps'] if s.get('name') == 'Install example dependencies and link local build')
            self.assertEqual(step['working-directory'], 'examples')
            for platform in ('Linux', 'Windows'):
                with self.subTest(job=job_name, platform=platform), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    (root / 'examples').mkdir()
                    (root / 'crates/bashkit-js').mkdir(parents=True)
                    (root / 'crates/bashkit-js/index.js').write_text('local release artifact')
                    (root / 'bin').mkdir()
                    pnpm = root / 'bin/pnpm'
                    pnpm.write_text('#!/bin/sh\n[ "$*" = "install --frozen-lockfile --ignore-scripts" ] || exit 42\nmkdir -p node_modules/@everruns/bashkit\nprintf stale > node_modules/@everruns/bashkit/index.js\n')
                    pnpm.chmod(0o755)
                    script = step['run'].replace('${{ github.workspace }}', str(root))
                    env = {**os.environ, 'RUNNER_OS': platform, 'PATH': str(root / 'bin') + os.pathsep + os.environ['PATH']}
                    result = subprocess.run(['/bin/bash', '-e', '-o', 'pipefail', '-c', script], cwd=root / 'examples', env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual((root / 'examples/node_modules/@everruns/bashkit/index.js').read_text(), 'local release artifact')

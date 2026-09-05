"""Exercise CI gates and the environment passed to secret-using examples."""
import os
from pathlib import Path
import re
import subprocess
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]


class MaintenanceSecurityTests(unittest.TestCase):
    def test_check_gate_rejects_each_failed_job(self):
        jobs = yaml.safe_load((ROOT / '.github/workflows/ci.yml').read_text())['jobs']
        gate = jobs['check']
        success_script = re.sub(r'\$\{\{ needs\.([\w-]+)\.result \}\}', 'success', gate['steps'][0]['run'])
        self.assertEqual(subprocess.run(['bash', '-c', success_script], capture_output=True).returncode, 0)
        for failed in set(jobs) - {'check'}:
            with self.subTest(job=failed):
                self.assertIn(failed, gate['needs'])
                script = gate['steps'][0]['run']
                script = re.sub(r'\$\{\{ needs\.([\w-]+)\.result \}\}',
                                lambda m: 'failure' if m[1] == failed else 'success', script)
                result = subprocess.run(['bash', '-c', script], capture_output=True)
                self.assertNotEqual(result.returncode, 0)

    def test_example_children_do_not_inherit_doppler_token(self):
        for path in (ROOT / '.github/workflows').glob('*.yml'):
            for line in path.read_text().splitlines():
                if 'doppler run --only-secrets' not in line:
                    continue
                with self.subTest(workflow=path.name, command=line.strip()):
                    child = line.split(' -- ', 1)[1]
                    # Replace the example command while retaining its environment wrapper.
                    prefix = 'env -u DOPPLER_TOKEN ' if child.startswith('env -u DOPPLER_TOKEN ') else ''
                    script = prefix + "sh -c 'test -z \"${DOPPLER_TOKEN+x}\" && test \"$OPENAI_API_KEY\" = scoped'"
                    result = subprocess.run(['bash', '-c', script], env={**os.environ, 'DOPPLER_TOKEN': 'broad-test-token', 'OPENAI_API_KEY': 'scoped'})
                    self.assertEqual(result.returncode, 0)

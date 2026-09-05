"""Exercise CI gates and the environment passed to secret-using examples."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
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

    def test_broad_credential_steps_never_execute_repository_code(self):
        for workflow in ('ci.yml', 'js.yml', 'publish-js.yml'):
            jobs = yaml.safe_load((ROOT / '.github/workflows' / workflow).read_text())['jobs']
            for job in jobs.values():
                self.assertNotIn('DOPPLER_TOKEN', job.get('env', {}))
                for step in job.get('steps', []):
                    if 'DOPPLER_TOKEN' not in step.get('env', {}):
                        continue
                    with self.subTest(workflow=workflow, step=step['name']):
                        script = step.get('run', '')
                        self.assertNotRegex(script, r'\b(cargo|node|bun|docker)\b|examples/')
                        self.assertNotIn('doppler run', script)
                        self.assertNotIn('uses', step)

    def test_optional_ci_examples_skip_failed_fetches(self):
        steps = yaml.safe_load((ROOT / '.github/workflows/ci.yml').read_text())['jobs']['examples']['steps']
        for ident, name in [('anthropic', 'Run LLM agent example'), ('openai', 'Run harness OpenAI joke example')]:
            fetch = next(step for step in steps if step.get('id') == ident)
            run = next(step for step in steps if step.get('name') == name)
            self.assertTrue(fetch['continue-on-error'])
            self.assertTrue(run['continue-on-error'])
            self.assertIn(f"steps.{ident}.outcome == 'success'", run['if'])

    def test_actual_workflow_scripts_separate_fetch_and_execution(self):
        """Execute checked-in scripts; replace executables, not shell wrappers."""
        for workflow in ('ci.yml', 'js.yml', 'publish-js.yml'):
            jobs = yaml.safe_load((ROOT / '.github/workflows' / workflow).read_text())['jobs']
            for name, job in jobs.items():
                fetches = [step for step in job.get('steps', [])
                           if 'doppler secrets get' in step.get('run', '')]
                if not fetches:
                    continue
                with self.subTest(workflow=workflow, job=name), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    (root / 'bin').mkdir()
                    log = root / 'calls.jsonl'
                    stub = root / 'bin' / 'doppler'
                    stub.write_text(f"#!{sys.executable}\n" + r'''
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
broad = 'DOPPLER_TOKEN' in os.environ
parent_env = pathlib.Path('/proc') / str(os.getppid()) / 'environ'
parent_broad = b'DOPPLER_TOKEN=' in parent_env.read_bytes() if parent_env.exists() else False
with open(os.environ['CALL_LOG'], 'a') as log:
    log.write(json.dumps({'name': name, 'args': sys.argv[1:], 'broad': broad,
        'parent_broad': parent_broad, 'pid': os.getpid()}) + '\n')
if name == 'doppler':
    assert broad
    assert sys.argv[1:3] == ['secrets', 'get'] and sys.argv[4:] == ['--plain']
    print('scoped-' + sys.argv[3])
else:
    assert not broad and not parent_broad
    for key in ('OPENAI_API_KEY', 'ANTHROPIC_API_KEY'):
        if key in os.environ:
            assert os.environ[key] == 'scoped-' + key
    if name == 'cargo':
        assert not any(key in os.environ for key in ('OPENAI_API_KEY', 'ANTHROPIC_API_KEY'))
    if name == 'docker':
        assert '-e' in sys.argv and 'OPENAI_API_KEY' in sys.argv
        assert not any('scoped-' in arg or arg.startswith('OPENAI_API_KEY=') for arg in sys.argv)
''')
                    stub.chmod(0o755)
                    for executable in ('node', 'bun', 'deno', 'cargo', 'bash'):
                        (root / 'bin' / executable).symlink_to(stub)
                    (root / 'bin' / 'docker').symlink_to(stub)
                    (root / 'target/debug/examples').mkdir(parents=True)
                    (root / 'target/debug/examples/agent_tool').symlink_to(stub)
                    base_env = {key: value for key, value in os.environ.items()
                                if key not in ('DOPPLER_TOKEN', 'OPENAI_API_KEY', 'ANTHROPIC_API_KEY')}
                    base_env.update(PATH=str(root / 'bin') + os.pathsep + base_env['PATH'], CALL_LOG=str(log))
                    outputs = {}
                    for step in job['steps']:
                        if step not in fetches and step.get('name') not in (
                            'Run AI examples', 'Run LLM agent example',
                            'Run harness OpenAI joke example', 'Build LLM examples without credentials'
                        ):
                            continue
                        output = root / 'output'
                        output.write_text('')
                        env = {**base_env, 'GITHUB_OUTPUT': str(output)}
                        for key, value in step.get('env', {}).items():
                            if key == 'DOPPLER_TOKEN':
                                env[key] = 'synthetic-broad-token'
                            else:
                                reference = re.fullmatch(r'\$\{\{ steps\.([\w-]+)\.outputs\.([\w-]+) \}\}', value)
                                self.assertIsNotNone(reference, value)
                                env[key] = outputs[reference.groups()]
                        script = step['run']
                        replacements = {'matrix.run': 'node', 'github.workspace': str(root),
                                        'steps.docker.outputs.PLATFORM': 'linux/amd64',
                                        'steps.docker.outputs.IMAGE': 'example-image'}
                        for expression, value in replacements.items():
                            script = script.replace('${{ ' + expression + ' }}', value)
                        self.assertNotIn('${{', script)
                        result = subprocess.run(['/bin/bash', '-e', '-o', 'pipefail', '-c', script],
                                                env=env, cwd=root, capture_output=True, text=True)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        for line in output.read_text().splitlines():
                            key, value = line.split('=', 1)
                            self.assertIn('::add-mask::' + value, result.stdout)
                            outputs[step['id'], key] = value
                    calls = [json.loads(line) for line in log.read_text().splitlines()]
                    self.assertTrue(any(call['name'] == 'doppler' for call in calls))
                    self.assertTrue(any(call['name'] != 'doppler' for call in calls))
                    for call in calls:
                        if call['name'] != 'doppler':
                            self.assertFalse(call['broad'])
                            self.assertFalse(call['parent_broad'])

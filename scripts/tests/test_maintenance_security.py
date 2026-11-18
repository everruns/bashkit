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
        for workflow in sorted((ROOT / '.github/workflows').glob('*.yml')):
            jobs = yaml.safe_load(workflow.read_text())['jobs']
            for job in jobs.values():
                self.assertNotIn('DOPPLER_TOKEN', job.get('env', {}))
                for step in job.get('steps', []):
                    if 'DOPPLER_TOKEN' not in step.get('env', {}):
                        continue
                    with self.subTest(workflow=workflow.name, step=step['name']):
                        script = step.get('run', '')
                        self.assertNotRegex(script, r'\b(cargo|node|bun|docker)\b|examples/')
                        self.assertNotRegex(script, r'(?m)^\s*(?:if\s+)?(?:git|gh)\s')
                        self.assertNotIn('doppler run', script)
                        self.assertNotIn('uses', step)

    def test_nightly_geiger_requires_a_valid_report_and_rust_coverage(self):
        jobs = yaml.safe_load((ROOT / '.github/workflows/nightly.yml').read_text())['jobs']
        steps = jobs['security-analysis']['steps']
        audit = next(step for step in steps if step.get('name') == 'Run cargo-geiger (unsafe code audit)')
        upload = next(step for step in steps if step.get('name') == 'Upload geiger report')
        self.assertEqual(audit.get('shell'), 'bash')
        self.assertFalse(audit.get('continue-on-error', False))
        self.assertEqual(upload['if'], 'always()')
        self.assertEqual(upload['with']['path'], 'geiger-*.json*')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cargo = root / 'cargo'
            cargo.write_text(
                '#!/bin/sh\n'
                '[ "$*" = "geiger --manifest-path $PWD/crates/bashkit/Cargo.toml --locked --all-features --output-format Json" ] || exit 64\n'
                'printf "%s\\n" "$GEIGER_OUTPUT"\n'
                'exit "$GEIGER_EXIT"\n'
            )
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'])
            report = {
                'packages': [{'package': {'id': {'name': 'bashkit'}}}],
                'packages_without_metrics': [],
                'used_but_not_scanned_files': [
                    str(root / 'target/debug/build/dependency/out/generated.rs'),
                    str(root / 'README.md'),
                ],
            }
            for name, output, exit_code, expected in (
                ('complete', report, 0, 0),
                ('geiger failure', report, 7, 7),
                ('no report', {}, 0, 5),
                ('invalid report', {'packages': report['packages']}, 0, 5),
                ('no Bashkit', dict(report, packages=[]), 0, 5),
                ('missing package metrics', dict(report, packages_without_metrics=['dep']), 0, 5),
                ('unscanned Rust input', dict(report, used_but_not_scanned_files=['/src/lib.rs']), 0, 5),
            ):
                with self.subTest(name=name):
                    raw_output = '{"$message_type":"artifact"}\n' + json.dumps(output)
                    result = subprocess.run(
                        ['bash', '-e', '-o', 'pipefail', '-c', audit['run']], cwd=root,
                        env=dict(env, GEIGER_OUTPUT=raw_output, GEIGER_EXIT=str(exit_code)),
                        capture_output=True, text=True,
                    )
                    self.assertEqual(result.returncode, expected, result.stderr)
                    self.assertEqual((root / 'geiger-output.jsonl').read_text(), raw_output + '\n')
                    if expected == 0:
                        self.assertEqual(json.loads((root / 'geiger-report.json').read_text()), report)

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
        for workflow in ('ci.yml', 'js.yml', 'publish-js.yml', 'cli-binaries.yml'):
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
# Match the injected token's value, not the bare variable name. bash may
# exec-replace itself for a step's final command, which reparents this stub
# onto the test runner; a real DOPPLER_TOKEN in the runner's own environment
# (the documented cloud agent env pre-configures one) then reads as a leak
# that never happened.
parent_env = pathlib.Path('/proc') / str(os.getppid()) / 'environ'
parent_broad = b'DOPPLER_TOKEN=synthetic-broad-token' in parent_env.read_bytes() if parent_env.exists() else False
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
    if name == 'git':
        import subprocess
        askpass = os.environ['GIT_ASKPASS']
        assert subprocess.check_output([askpass, 'Password:'], text=True).strip() == 'scoped-HOMEBREW_TAP_GITHUB_TOKEN'
        if sys.argv[1] == 'clone':
            pathlib.Path('tap/Formula').mkdir(parents=True)
        if sys.argv[1] == 'diff':
            sys.exit(1)
''')
                    stub.chmod(0o755)
                    for executable in ('node', 'bun', 'deno', 'cargo', 'bash', 'git'):
                        (root / 'bin' / executable).symlink_to(stub)
                    (root / 'bin' / 'docker').symlink_to(stub)
                    (root / 'target/debug/examples').mkdir(parents=True)
                    (root / 'target/debug/examples/agent_tool').symlink_to(stub)
                    (root / 'bashkit.rb').write_text('fixture formula\n')
                    base_env = {key: value for key, value in os.environ.items()
                                if key not in ('DOPPLER_TOKEN', 'OPENAI_API_KEY', 'ANTHROPIC_API_KEY', 'GH_PAT')}
                    base_env.update(PATH=str(root / 'bin') + os.pathsep + base_env['PATH'], CALL_LOG=str(log))
                    outputs = {}
                    for step in job['steps']:
                        if step not in fetches and step.get('name') not in (
                            'Run AI examples', 'Run LLM agent example',
                            'Run harness OpenAI joke example', 'Build LLM examples without credentials',
                            'Push formula to homebrew-tap',
                        ):
                            continue
                        output = root / 'output'
                        output.write_text('')
                        env = {**base_env, 'GITHUB_OUTPUT': str(output)}
                        for key, value in step.get('env', {}).items():
                            if key == 'DOPPLER_TOKEN':
                                env[key] = 'synthetic-broad-token'
                            elif key == 'RELEASE_TAG' and value == '${{ needs.validate-tag.outputs.tag }}':
                                env[key] = 'v1.2.3'
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
                    if workflow == 'cli-binaries.yml':
                        self.assertIn(['push', 'origin', 'main'],
                                      [call['args'] for call in calls if call['name'] == 'git'])
                        self.assertEqual((root / 'tap/Formula/bashkit.rb').read_text(), 'fixture formula\n')
                    for call in calls:
                        if call['name'] != 'doppler':
                            self.assertFalse(call['broad'])
                            self.assertFalse(call['parent_broad'])

"""Keep CI credentials read-only and release examples on reviewed dependencies."""
import os
from pathlib import Path
import re
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


class CheckoutCredentialTests(unittest.TestCase):
    """TM-INF-026: a checkout token left in `.git/config` is a live credential.

    `actions/checkout` writes the job's `GITHUB_TOKEN` into
    `http.<host>.extraheader` unless told not to. Anything the job then runs
    (a dependency `build.rs`, an npm lifecycle script, a test) can read it.
    The rule is derived, not allowlisted: a job may keep the credential only
    when it actually authenticates a `git`/`gh` call, and never when the
    workflow can be triggered by a pull request.
    """

    GIT_CALL = re.compile(r'(?:^|[\s;&|(`])(?:git|gh)\s+[a-z-]', re.M)

    @staticmethod
    def workflows():
        for path in sorted((ROOT / '.github/workflows').glob('*.yml')):
            yield path, yaml.safe_load(path.read_text())

    @staticmethod
    def triggers(workflow):
        # PyYAML resolves the bare `on:` key to the boolean True.
        return workflow.get('on', workflow.get(True)) or {}

    @staticmethod
    def checkouts(job):
        return [step for step in job.get('steps', []) or []
                if str(step.get('uses', '')).startswith('actions/checkout@')]

    @classmethod
    def persisting(cls, job):
        return [step for step in cls.checkouts(job)
                if step.get('with', {}).get('persist-credentials') is not False]

    def test_checkout_keeps_credentials_only_where_git_authenticates(self):
        for path, workflow in self.workflows():
            for name, job in (workflow.get('jobs') or {}).items():
                if not self.persisting(job):
                    continue
                script = '\n'.join(step.get('run', '') for step in job.get('steps', []) or [])
                with self.subTest(workflow=path.name, job=name):
                    self.assertRegex(
                        script, self.GIT_CALL,
                        f'{path.name}:{name} persists the checkout token but runs no git/gh '
                        'command; add `persist-credentials: false`')

    def test_pull_request_jobs_never_persist_the_checkout_token(self):
        for path, workflow in self.workflows():
            if 'pull_request' not in self.triggers(workflow):
                continue
            for name, job in (workflow.get('jobs') or {}).items():
                with self.subTest(workflow=path.name, job=name):
                    self.assertEqual(
                        [], self.persisting(job),
                        f'{path.name}:{name} runs pull-request-authored code with the '
                        'checkout token in .git/config')

    def test_every_workflow_defaults_to_read_only_permissions(self):
        for path, workflow in self.workflows():
            with self.subTest(workflow=path.name):
                permissions = workflow.get('permissions')
                self.assertIsNotNone(
                    permissions, f'{path.name} inherits repository default permissions')
                self.assertNotIn('write', permissions.values())


class SecretHandlingTests(unittest.TestCase):
    """Runtime-fetched secrets are not registered with the log masker.

    A value pulled from Doppler during a job is invisible to GitHub's
    scrubber until `::add-mask::` announces it, and `$GITHUB_ENV` republishes
    it in the environment block of every later step. So: mask first, hand it
    on through `$GITHUB_OUTPUT`, and never through `$GITHUB_ENV`.
    """

    FETCH = re.compile(r'^\s*(\w+)=\$\(doppler secrets get\b', re.M)

    @staticmethod
    def steps():
        for path in sorted((ROOT / '.github/workflows').glob('*.yml')):
            workflow = yaml.safe_load(path.read_text())
            for name, job in (workflow.get('jobs') or {}).items():
                for step in job.get('steps', []) or []:
                    if step.get('run'):
                        yield path.name, name, step

    def test_github_env_only_carries_secret_presence_booleans(self):
        for workflow, job, step in self.steps():
            for line in step['run'].split('\n'):
                if 'GITHUB_ENV' not in line or 'secrets.' not in line:
                    continue
                with self.subTest(workflow=workflow, job=job, line=line.strip()):
                    self.assertRegex(
                        line, r"secrets\.\w+\s*(?:!=|==)\s*''",
                        'only a presence comparison may reach $GITHUB_ENV, never a secret value')

    def test_fetched_secrets_never_reach_github_env(self):
        for workflow, job, step in self.steps():
            if 'doppler secrets get' not in step['run']:
                continue
            with self.subTest(workflow=workflow, job=job, step=step.get('name')):
                self.assertNotIn(
                    'GITHUB_ENV', step['run'],
                    'a Doppler-fetched value in $GITHUB_ENV is unmasked in every later step')

    def test_fetched_secrets_are_masked_before_they_travel(self):
        for workflow, job, step in self.steps():
            lines = step['run'].split('\n')
            for match in self.FETCH.finditer(step['run']):
                variable = match.group(1)
                fetched = step['run'][:match.start()].count('\n')
                masked = next((i for i, line in enumerate(lines)
                               if i > fetched and '::add-mask::' in line and variable in line), None)
                with self.subTest(workflow=workflow, job=job, variable=variable):
                    self.assertIsNotNone(masked, f'${variable} is never passed to ::add-mask::')
                    travels = next((i for i, line in enumerate(lines)
                                    if i > fetched and variable in line
                                    and ('GITHUB_OUTPUT' in line or re.search(rf'\bexport\s+{variable}\b', line))),
                                   None)
                    if travels is not None:
                        self.assertLess(masked, travels,
                                        f'${variable} escapes the fetch step before it is masked')

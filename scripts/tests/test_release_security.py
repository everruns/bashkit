"""Release source identity and compiler credential isolation regressions."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]


def jobs(name):
    return yaml.safe_load((ROOT / '.github/workflows' / name).read_text())['jobs']


class ReleaseSecurityTests(unittest.TestCase):
    def test_binary_builds_use_validated_immutable_commit(self):
        for name in ('c-api-binaries.yml', 'cli-binaries.yml'):
            with self.subTest(workflow=name):
                workflow = jobs(name)
                validator = workflow['validate-tag']
                source = next(s for s in validator['steps'] if s.get('name') == 'Verify tag source')
                self.assertEqual(validator['outputs']['sha'], '${{ steps.source.outputs.sha }}')
                self.assertEqual(source['id'], 'source')
                checkout = next(s for s in workflow['build']['steps'] if s.get('uses', '').startswith('actions/checkout@'))
                self.assertEqual(checkout['with']['ref'], '${{ needs.validate-tag.outputs.sha }}')
                # Re-fetch can move a tag. Version validation must read that same
                # resolved tree, never the earlier checkout's Cargo.toml.
                script = source['run']
                self.assertLess(script.index('git fetch'), script.index('git checkout --detach "$TAG_SHA"'))
                self.assertLess(script.index('git checkout --detach "$TAG_SHA"'), script.index('CARGO_VERSION='))
                self.assertGreater(script.index('echo "sha=$TAG_SHA"'), script.index('git merge-base'))

    def test_validation_reads_refetched_tree_and_returns_stable_sha(self):
        for name in ('c-api-binaries.yml', 'cli-binaries.yml'):
            with self.subTest(workflow=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                origin = root / 'origin'
                checkout = root / 'checkout'
                def git(*args, cwd=origin):
                    return subprocess.check_output(['git', *args], cwd=cwd, stderr=subprocess.DEVNULL, text=True).strip()
                origin.mkdir()
                git('init', '-b', 'main')
                git('config', 'user.name', 'Release test fixture')
                git('config', 'user.email', 'fixture@example.invalid')
                (origin / 'Cargo.toml').write_text('version = "1.2.3"\n')
                git('add', '.')
                git('commit', '-m', 'valid release')
                valid = git('rev-parse', 'HEAD')
                git('tag', 'v1.2.3')
                git('clone', str(origin), str(checkout), cwd=root)
                script = next(s['run'] for s in jobs(name)['validate-tag']['steps'] if s.get('name') == 'Verify tag source')
                output = root / 'outputs'
                env = dict(os.environ, RELEASE_TAG='v1.2.3', GITHUB_SHA=valid, GITHUB_OUTPUT=str(output))
                result = subprocess.run(['bash', '-e', '-c', script], cwd=checkout, env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f'sha={valid}', output.read_text())
                # Remote tag now points at a different version; the old checkout
                # still has the valid manifest, so validating it would be a bypass.
                (origin / 'Cargo.toml').write_text('version = "9.9.9"\n')
                git('commit', '-am', 'different source')
                moved = git('rev-parse', 'HEAD')
                git('tag', '-f', 'v1.2.3')
                env['GITHUB_SHA'] = moved
                output.unlink()
                result = subprocess.run(['bash', '-e', '-c', script], cwd=checkout, env=env, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('does not match Cargo.toml', result.stderr)
                self.assertFalse(output.exists())
                # The successful validation's exported SHA still builds old source.
                git('checkout', '--detach', valid, cwd=checkout)
                self.assertEqual((checkout / 'Cargo.toml').read_text(), 'version = "1.2.3"\n')

    def test_verification_compiler_never_inherits_registry_token(self):
        for name in ('publish-bashkit', 'publish-bashkit-cli'):
            with self.subTest(job=name), tempfile.TemporaryDirectory() as directory:
                job = jobs('publish.yml')[name]
                steps = [s for s in job['steps'] if 'cargo publish' in s.get('run', '')]
                self.assertEqual(len(steps), 2)
                self.assertNotIn('CARGO_REGISTRY_TOKEN', job.get('env', {}))
                root = Path(directory)
                cargo = root / 'cargo'
                cargo.write_text('#!/usr/bin/env python3\nimport json,os,sys\nwith open(os.environ["TRACE"], "a") as f: f.write(json.dumps({"args":sys.argv[1:],"token":os.environ.get("CARGO_REGISTRY_TOKEN")})+"\\n")\n')
                cargo.chmod(0o755)
                trace = root / 'trace'
                for step in steps:
                    env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', TRACE=str(trace))
                    env.pop('CARGO_REGISTRY_TOKEN', None)
                    if 'CARGO_REGISTRY_TOKEN' in step.get('env', {}):
                        env['CARGO_REGISTRY_TOKEN'] = 'sentinel-registry-secret'
                    subprocess.run(['bash', '-e', '-c', step['run']], env=env, check=True)
                calls = [json.loads(line) for line in trace.read_text().splitlines()]
                self.assertIn('--dry-run', calls[0]['args'])
                self.assertNotIn('--no-verify', calls[0]['args'])
                self.assertIsNone(calls[0]['token'])
                self.assertIn('--no-verify', calls[1]['args'])
                self.assertNotIn('--dry-run', calls[1]['args'])
                self.assertEqual(calls[1]['token'], 'sentinel-registry-secret')


if __name__ == '__main__':
    unittest.main()

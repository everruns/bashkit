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
        for name in ('c-api-binaries.yml', 'cli-binaries.yml', 'publish.yml'):
            with self.subTest(workflow=name):
                workflow = jobs(name)
                validator = workflow['validate-tag']
                source = next(s for s in validator['steps'] if s.get('name') == 'Verify tag source')
                self.assertEqual(validator['outputs']['sha'], '${{ steps.source.outputs.sha }}')
                self.assertEqual(source['id'], 'source')
                builds = [job for key, job in workflow.items() if key not in ('validate-tag', 'verify-publish')] if name == 'publish.yml' else [workflow['build']]
                for build in builds:
                    checkout = next(s for s in build['steps'] if s.get('uses', '').startswith('actions/checkout@'))
                    self.assertEqual(checkout['with']['ref'], '${{ needs.validate-tag.outputs.sha }}')
                    self.assertIn('validate-tag', build['needs'])
                # Re-fetch can move a tag. Version validation must read that same
                # resolved tree, never the earlier checkout's Cargo.toml.
                script = source['run']
                self.assertLess(script.index('git fetch'), script.index('git checkout --detach "$TAG_SHA"'))
                self.assertLess(script.index('git checkout --detach "$TAG_SHA"'), script.index('CARGO_VERSION='))
                self.assertGreater(script.index('echo "sha=$TAG_SHA"'), script.index('git merge-base'))

    def test_validation_reads_refetched_tree_and_returns_stable_sha(self):
        for name in ('c-api-binaries.yml', 'cli-binaries.yml', 'publish.yml'):
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
                env = dict(os.environ, RELEASE_TAG='v1.2.3', GITHUB_SHA=valid, GITHUB_REF='refs/tags/v1.2.3', GITHUB_OUTPUT=str(output))
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

    def test_crates_recovery_builds_original_tag_from_current_main_only(self):
        workflow = jobs('publish.yml')['validate-tag']
        script = next(s['run'] for s in workflow['steps'] if s.get('name') == 'Verify tag source')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            origin = root / 'origin'
            checkout = root / 'checkout'
            origin.mkdir()
            def git(*args, cwd=origin):
                return subprocess.check_output(['git', *args], cwd=cwd, stderr=subprocess.DEVNULL, text=True).strip()
            git('init', '-b', 'main')
            git('config', 'user.name', 'Release test fixture')
            git('config', 'user.email', 'fixture@example.invalid')
            (origin / 'Cargo.toml').write_text('version = "1.2.3"\n')
            git('add', '.')
            git('commit', '-m', 'release')
            release = git('rev-parse', 'HEAD')
            git('tag', 'v1.2.3')
            # Main can advance its manifest; recovery must still read the old tag.
            (origin / 'Cargo.toml').write_text('version = "1.2.4"\n')
            git('commit', '-am', 'workflow repair')
            current = git('rev-parse', 'HEAD')
            git('clone', str(origin), str(checkout), cwd=root)
            output = root / 'outputs'
            env = dict(os.environ, RELEASE_TAG='v1.2.3', GITHUB_SHA=current,
                       GITHUB_REF='refs/heads/main', GITHUB_OUTPUT=str(output))
            result = subprocess.run(['bash', '-e', '-c', script], cwd=checkout, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(f'sha={release}', output.read_text())
            self.assertEqual(git('rev-parse', 'HEAD', cwd=checkout), release)
            for ref, sha in (('refs/heads/other', current), ('refs/heads/main', release),
                             ('refs/tags/v9.9.9', release)):
                output.unlink(missing_ok=True)
                env.update(GITHUB_REF=ref, GITHUB_SHA=sha)
                result = subprocess.run(['bash', '-e', '-c', script], cwd=checkout, env=env, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(output.exists())

    def test_registry_verification_uses_workflow_code_and_validated_version(self):
        job = jobs('publish.yml')['verify-publish']
        checkout = next(s for s in job['steps'] if s.get('uses', '').startswith('actions/checkout@'))
        self.assertEqual(checkout['with']['ref'], '${{ github.sha }}')
        verify = next(s for s in job['steps'] if s.get('name') == 'Verify crates.io versions')
        self.assertEqual(verify['env']['RELEASE_TAG'], '${{ needs.validate-tag.outputs.tag }}')
        self.assertIn('EXPECTED="${RELEASE_TAG#v}"', verify['run'])
        self.assertNotIn('Cargo.toml', verify['run'])

    def test_crates_release_tag_input_resolution(self):
        script = next(s['run'] for s in jobs('publish.yml')['validate-tag']['steps']
                      if s.get('name') == 'Validate workflow input')
        for event, release, input_tag, ref, expected in (
            ('workflow_dispatch', '', 'v1.2.3', 'refs/heads/main', 'v1.2.3'),
            ('workflow_dispatch', '', '', 'refs/tags/v1.2.3', 'v1.2.3'),
            ('release', 'v1.2.3', '', 'refs/tags/v1.2.3', 'v1.2.3'),
            ('workflow_dispatch', '', '', 'refs/heads/main', None),
            ('workflow_dispatch', '', 'v1.2.3-rc1', 'refs/heads/main', None),
        ):
            with self.subTest(event=event, input=input_tag, ref=ref), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / 'outputs'
                env = dict(os.environ, EVENT_NAME=event, RELEASE_TAG=release, INPUT_TAG=input_tag,
                           GITHUB_REF=ref, GITHUB_OUTPUT=str(output))
                result = subprocess.run(['bash', '-e', '-c', script], env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, expected is not None, result.stderr)
                if expected:
                    self.assertIn(f'tag={expected}', output.read_text())

    def test_cpython_size_gate_checks_a_packaged_archive(self):
        step = next(s for s in jobs('publish.yml')['publish-bashkit-cpython-wasm']['steps']
                    if s.get('name') == 'Verify bashkit-cpython-wasm package without credentials')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cargo = root / 'cargo'
            # Cargo 1.95 publish dry-runs leave their archive in tmp-crate;
            # only `cargo package` finalizes the target/package archive.
            cargo.write_text("#!/usr/bin/env python3\nimport os,sys\nfrom pathlib import Path\nassert 'CARGO_REGISTRY_TOKEN' not in os.environ\nif sys.argv[1] == 'package':\n    assert '--no-verify' not in sys.argv\n    path=Path(os.environ['CARGO_TARGET_DIR']) / 'package/bashkit-cpython-wasm-1.2.3.crate'\nelse:\n    assert sys.argv[1] == 'publish' and '--dry-run' in sys.argv\n    path=Path(os.environ['CARGO_TARGET_DIR']) / 'package/tmp-crate/bashkit-cpython-wasm-1.2.3.crate'\npath.parent.mkdir(parents=True,exist_ok=True)\nwith path.open('wb') as archive:\n    archive.truncate(int(os.environ['ARCHIVE_SIZE']))\n")
            cargo.chmod(0o755)
            (root / 'Cargo.toml').write_text('version = "1.2.3"\n')
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}')
            env.pop('CARGO_REGISTRY_TOKEN', None)
            env['CARGO_TARGET_DIR'] = str(root / 'target')
            for size, accepted in ((10485759, True), (10485760, False)):
                with self.subTest(size=size):
                    env['ARCHIVE_SIZE'] = str(size)
                    result = subprocess.run(['bash', '-e', '-c', step['run']], cwd=root,
                                            env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode == 0, accepted, result.stderr)
                    if not accepted:
                        self.assertIn('exceeds the crates.io 10 MiB limit', result.stdout)

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

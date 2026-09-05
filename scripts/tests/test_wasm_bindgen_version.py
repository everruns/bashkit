"""Bindgen CLI schema must follow the checked-in Cargo.lock, including caches."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
INSTALLER = 'scripts/install-wasm-bindgen.sh'


class WasmBindgenVersionTests(unittest.TestCase):
    def test_ci_and_publish_use_lock_derived_installer(self):
        for name in ('ci.yml', 'publish-wasm.yml'):
            with self.subTest(workflow=name):
                workflow = yaml.safe_load((ROOT / '.github/workflows' / name).read_text())
                self.assertNotIn('WASM_BINDGEN_VERSION', workflow.get('env', {}))
                steps = [step for job in workflow['jobs'].values() for step in job.get('steps', [])
                         if step.get('name') == 'Install wasm-bindgen-cli']
                self.assertEqual(len(steps), 1)
                self.assertEqual(steps[0]['run'], f'bash {INSTALLER}')

    def test_installer_replaces_stale_cache_and_reuses_matching_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'scripts').mkdir()
            shutil.copy(ROOT / INSTALLER, root / INSTALLER)
            (root / 'Cargo.lock').write_text('version = 4\n[[package]]\nname = "wasm-bindgen"\nversion = "0.2.999"\n')
            (root / 'bin').mkdir()
            state = root / 'version'
            state.write_text('0.2.126')
            log = root / 'cargo-log'
            (root / 'bin/wasm-bindgen').write_text('#!/bin/sh\nprintf "wasm-bindgen %s\\n" "$(cat "$STATE")"\n')
            (root / 'bin/cargo').write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$LOG"\nprintf 0.2.999 > "$STATE"\n')
            for command in (root / 'bin').iterdir():
                command.chmod(0o755)
            env = dict(os.environ, PATH=f'{root / "bin"}:{os.environ["PATH"]}', STATE=str(state), LOG=str(log))
            for _ in range(2):
                result = subprocess.run(['bash', str(root / INSTALLER)], env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(log.read_text().splitlines(), ['install wasm-bindgen-cli --version 0.2.999 --locked --force'])
            # A shadowing old binary must fail the post-install check.
            state.write_text('0.2.126')
            (root / 'bin/cargo').write_text('#!/bin/sh\nexit 0\n')
            result = subprocess.run(['bash', str(root / INSTALLER)], env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('does not match Cargo.lock', result.stderr)
            for lock in ('version = 4\npackage = []\n',
                         '[[package]]\nname = "wasm-bindgen"\nversion = "0.2.1"\n[[package]]\nname = "wasm-bindgen"\nversion = "0.2.2"\n'):
                (root / 'Cargo.lock').write_text(lock)
                result = subprocess.run(['bash', str(root / INSTALLER)], env=env, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('exactly one wasm-bindgen version', result.stderr)



if __name__ == '__main__':
    unittest.main()

"""Run the checked-in SQLite bootstrap with an isolated executable search path."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]


class SqliteBootstrapTests(unittest.TestCase):
    def test_install_only_when_sqlite_is_missing(self):
        jobs = yaml.safe_load((ROOT / '.github/workflows/ci.yml').read_text())['jobs']
        step = next(step for job in jobs.values() for step in job.get('steps', [])
                    if step.get('name') == 'Install host sqlite3 for differential tests')
        for present in (True, False):
            with self.subTest(present=present), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                log = root / 'install.log'
                sqlite = root / 'sqlite3'
                sqlite_script = '#!/bin/sh\nprintf "3.test\\n"\n'
                if present:
                    sqlite.write_text(sqlite_script)
                    sqlite.chmod(0o755)
                which = root / 'which'
                which.write_text('#!/bin/sh\ncommand -v "$1"\n')
                which.chmod(0o755)
                sudo = root / 'sudo'
                sudo.write_text(f'#!{sys.executable}\n' + f'''
from pathlib import Path
import sys
with Path({str(log)!r}).open('a') as log:
    log.write(' '.join(sys.argv[1:]) + '\\n')
if sys.argv[1:3] == ['apt-get', 'install']:
    path = Path({str(sqlite)!r})
    path.write_text({sqlite_script!r})
    path.chmod(0o755)
''')
                sudo.chmod(0o755)
                result = subprocess.run(['/bin/bash', '-e', '-o', 'pipefail', '-c', step['run']],
                                        env={**os.environ, 'PATH': str(root)}, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn('3.test', result.stdout)
                installs = log.read_text().splitlines() if log.exists() else []
                self.assertEqual(installs, [] if present else ['apt-get update', 'apt-get install -y sqlite3'])

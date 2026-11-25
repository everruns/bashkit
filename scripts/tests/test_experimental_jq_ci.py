"""The owned jq backend must execute the same native gates as the stable one."""
import importlib.util
from pathlib import Path
import tomllib
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('jq_fuzz_smoke', ROOT / 'scripts/fuzz-smoke.py')
fuzz = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fuzz)


def jobs(workflow):
    return yaml.safe_load((ROOT / '.github/workflows' / workflow).read_text())['jobs']


def commands(job):
    return '\n'.join(str(step.get('run', '')) for step in job['steps'])


class ExperimentalJqCI(unittest.TestCase):
    def test_every_embedding_forwards_the_opt_in_feature(self):
        for crate in ['cli', 'capi', 'python', 'js', 'wasm', 'scripted-tool', 'bench', 'eval']:
            with self.subTest(crate=crate):
                manifest = tomllib.loads((ROOT / f'crates/bashkit-{crate}/Cargo.toml').read_text())
                self.assertEqual(manifest.get('features', {}).get('experimental_jq'),
                                 ['bashkit/experimental_jq'])
                self.assertNotIn('experimental_jq', manifest.get('features', {}).get('default', []))

    def test_required_native_matrix_executes_both_backends_and_plans(self):
        ci = jobs('ci.yml')
        job = ci['jq-native']
        matrix = job['strategy']['matrix']
        self.assertEqual(set(matrix['experimental_jq']), {False, True})
        self.assertEqual(set(matrix['execution_plan']), {False, True})
        self.assertEqual(set(matrix['os']), {'ubuntu-24.04', 'macos-latest', 'windows-latest'})
        run = commands(job)
        for token in ['--lib --test integration', 'experimental_jq', 'experimental_execution_plan',
                      'security_failpoint_tests', '--test-threads=1', 'jq yq']:
            self.assertIn(token, run)
        self.assertIn('jq-native', ci['check']['needs'])
        self.assertIn('needs.jq-native.result', commands(ci['check']))

    def test_fuzz_smoke_and_scheduled_fuzzing_select_owned_rows(self):
        rows = fuzz.matrix()
        for target in ['jq_fuzz', 'yq_fuzz', 'jq_lifecycle_fuzz']:
            with self.subTest(target=target):
                selected = [row for row in rows if row['target'] == target]
                self.assertEqual({(row['execution_plan'], row.get('experimental_jq', False))
                                  for row in selected},
                                 {(False, False), (False, True), (True, False), (True, True)})
                for row in selected:
                    self.assertEqual('experimental_jq' in row['features'].split(','),
                                     row['experimental_jq'])
        smoke = jobs('ci.yml')['fuzz-check']
        self.assertIn(True, smoke['strategy']['matrix']['experimental_jq'])
        self.assertIn('--experimental-jq', commands(smoke))
        scheduled = jobs('fuzz.yml')
        self.assertIn('scripts/fuzz-smoke.py --matrix', commands(scheduled['targets']))
        self.assertIn('matrix.experimental_jq', str(scheduled['fuzz']))

    def test_sanitizers_miri_and_coverage_exercise_owned_storage(self):
        nightly = jobs('nightly.yml')
        for name in ['asan', 'miri']:
            self.assertIn('experimental_jq', commands(nightly[name]))
        self.assertIn('owning_iterator_keeps_storage_charged_until_drop', commands(nightly['miri']))
        coverage = jobs('coverage.yml')['coverage']
        self.assertEqual(set(coverage['strategy']['matrix']['experimental_jq']), {False, True})
        self.assertIn('experimental_jq', commands(coverage))

    def test_binding_and_browser_binaries_run_the_owned_backend(self):
        for workflow in ['ci.yml', 'python.yml', 'js.yml']:
            text = (ROOT / '.github/workflows' / workflow).read_text()
            self.assertIn('bashkit/experimental_execution_plan,bashkit/experimental_jq', text)


if __name__ == '__main__':
    unittest.main()

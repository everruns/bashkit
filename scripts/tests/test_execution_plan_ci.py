"""Guard against feature-on CI that merely compiles or omits security paths."""
from pathlib import Path
import importlib.util
import tomllib
import unittest
import yaml

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('fuzz_smoke', ROOT / 'scripts/fuzz-smoke.py')
fuzz = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fuzz)


def modes(job):
    matrix = job.get('strategy', {}).get('matrix', {})
    return set(matrix.get('execution_plan', [])) | {
        row['execution_plan'] for row in matrix.get('include', []) if 'execution_plan' in row
    }


class UniqueKeys(yaml.SafeLoader):
    def construct_mapping(self, node, deep=False):
        keys = [key.value for key, _ in node.value]
        if len(keys) != len(set(keys)):
            raise ValueError('duplicate YAML key: ' + str(keys))
        return super().construct_mapping(node, deep=deep)


class ExecutionPlanCI(unittest.TestCase):
    def test_workflow_mappings_do_not_shadow_env_or_other_keys(self):
        for workflow in (ROOT / '.github/workflows').glob('*.yml'):
            with self.subTest(workflow=workflow.name):
                yaml.load(workflow.read_text(), Loader=UniqueKeys)

    def test_full_core_and_security_suites_keep_both_engines(self):
        jobs = yaml.safe_load((ROOT / '.github/workflows/ci.yml').read_text())['jobs']
        self.assertEqual(modes(jobs['test']), {False, True})
        commands = '\n'.join(str(step.get('run', '')) for step in jobs['test']['steps'])
        for line in commands.splitlines():
            if 'cargo test ' in line:
                self.assertIn('experimental_execution_plan', line)
        for suite in ['--workspace --lib --bins --tests', '--doc', 'security_failpoint_tests',
                      'proptest_security', '--lib --test security_failpoint_tests']:
            self.assertIn(suite, commands)
        self.assertEqual(modes(jobs['fuzz-check']), {False, True})
        self.assertTrue(any('scripts/fuzz-smoke.py' in str(s.get('run', ''))
                            for s in jobs['fuzz-check']['steps']))
        self.assertIn('test', jobs['check']['needs'])
        self.assertIn('fuzz-check', jobs['check']['needs'])

    def test_cargo_fuzz_features_use_one_comma_separated_option(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        for line in workflow.splitlines():
            if ' fuzz build ' in line:
                self.assertEqual(line.count('--features'), 1, line)
                self.assertIn('--codegen-units 16', line)

    def test_each_fuzzer_runs_in_each_supported_mode_with_required_features(self):
        manifest = tomllib.loads((ROOT / 'crates/bashkit/fuzz/Cargo.toml').read_text())
        rows = fuzz.matrix()
        for target in manifest['bin']:
            selected = [r for r in rows if r['target'] == target['name']]
            required = set(target.get('required-features', []))
            expected = {True} if 'plan_differential' in required else {False, True}
            self.assertEqual({r['execution_plan'] for r in selected}, expected)
            for row in selected:
                features = set(filter(None, row['features'].split(',')))
                self.assertLessEqual(required, features)
                self.assertEqual('experimental_execution_plan' in features, row['execution_plan'])
        self.assertIn('bashkit/experimental_execution_plan', manifest['features']['experimental_execution_plan'])

    def test_binding_artifacts_and_sanitizers_cover_both_modes(self):
        for workflow, names in {
            'ci.yml': ['wasm-web', 'windows-containment', 'test-windows', 'cpython-platforms'],
            'python.yml': ['test', 'build-wheel', 'callback-platforms'],
            'js.yml': ['build-and-test', 'callback-platforms'],
            'coverage.yml': ['coverage'],
            'nightly.yml': ['asan', 'miri', 'python-stress-soak', 'ts-stress-soak'],
            'windows-nightly.yml': ['windows-tests'],
            'bash-oracle.yml': ['scoreboard'],
        }.items():
            jobs = yaml.safe_load((ROOT / '.github/workflows' / workflow).read_text())['jobs']
            for name in names:
                with self.subTest(workflow=workflow, job=name):
                    self.assertEqual(modes(jobs[name]), {False, True})
        python = (ROOT / '.github/workflows/python.yml').read_text()
        self.assertEqual(python.count('name: python-wheel-${{ matrix.execution_plan }}'), 2)

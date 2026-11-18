"""Cover the release gate with GitHub Actions run records."""

from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_nightly


def run(status='completed', conclusion='success'):
    return {'status': status, 'conclusion': conclusion, 'url': 'https://example.test/run'}


class NightlyGateTests(unittest.TestCase):
    def check(self, nightly_runs, fuzz_runs):
        responses = [
            subprocess.CompletedProcess([], 0, stdout=json.dumps(runs))
            if isinstance(runs, list) else runs
            for runs in (nightly_runs, fuzz_runs)
        ]
        output = io.StringIO()
        with patch.object(check_nightly.subprocess, 'run', side_effect=responses) as gh:
            with redirect_stdout(output):
                result = check_nightly.main()
        self.assertEqual([call.args[0] for call in gh.call_args_list], [
            ['gh', 'run', 'list', '--workflow', workflow, '--branch', 'main',
             '--limit', '3', '--json', 'status,conclusion,url']
            for workflow in ('nightly.yml', 'fuzz.yml')
        ])
        return result, output.getvalue()

    def test_three_completed_successes_per_workflow_pass(self):
        result, output = self.check([run()] * 3, [run()] * 3)
        self.assertEqual(result, 0, output)

    def test_empty_and_partial_history_block_release(self):
        for index, workflow in enumerate(('nightly.yml', 'fuzz.yml')):
            for count in range(3):
                with self.subTest(workflow=workflow, count=count):
                    histories = [[run()] * 3, [run()] * 3]
                    histories[index] = [run()] * count
                    result, output = self.check(*histories)
                    self.assertEqual(result, 1, output)
                    self.assertIn(f'{workflow}: expected 3 runs, got {count}', output)

    def test_malformed_record_blocks_release_and_keeps_other_run_links(self):
        for malformed in (None, {'status': 'completed'}, dict(run(), url=42)):
            with self.subTest(malformed=malformed):
                result, output = self.check([run(), malformed, run()], [run()] * 3)
                self.assertEqual(result, 1, output)
                self.assertIn('nightly.yml: malformed run 2', output)
                self.assertIn('fuzz.yml', output)
                self.assertEqual(output.count(run()['url']), 5)

    def test_non_successful_runs_block_release_and_show_the_run_url(self):
        for record, label in ((run('in_progress', ''), 'in_progress'),
                              (run(conclusion='failure'), 'failure'),
                              (run(conclusion='cancelled'), 'cancelled')):
            with self.subTest(label=label):
                result, output = self.check([run()] * 3, [record, run(), run()])
                self.assertEqual(result, 1, output)
                self.assertIn(f'{label} {record["url"]}', output)

    def test_api_error_blocks_release_and_does_not_hide_other_workflow(self):
        result, output = self.check(subprocess.CalledProcessError(1, ['gh']), [run()] * 3)
        self.assertEqual(result, 1, output)
        self.assertIn('nightly.yml: unable to read runs', output)
        self.assertIn('fuzz.yml', output)


if __name__ == '__main__':
    unittest.main()

"""Require three green Nightly and Fuzz Testing runs on main before release."""

import json
import subprocess

REQUIRED_RUNS = 3


def check_workflow(workflow):
    print(f'\n=== {workflow} ===')
    try:
        response = subprocess.run(
            ['gh', 'run', 'list', '--workflow', workflow, '--branch', 'main',
             '--limit', str(REQUIRED_RUNS), '--json', 'status,conclusion,url'],
            check=True, stdout=subprocess.PIPE, text=True,
        )
        runs = json.loads(response.stdout)
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print(f'ERROR: {workflow}: unable to read runs ({error})')
        return False

    if not isinstance(runs, list):
        print(f'ERROR: {workflow}: expected a list of runs')
        return False

    passed = len(runs) == REQUIRED_RUNS
    if not passed:
        print(f'ERROR: {workflow}: expected {REQUIRED_RUNS} runs, got {len(runs)}')
    for index, run in enumerate(runs, start=1):
        if (not isinstance(run, dict)
                or not all(isinstance(run.get(key), str) for key in ('status', 'conclusion', 'url'))
                or not run['status'] or not run['url']):
            print(f'ERROR: {workflow}: malformed run {index}')
            passed = False
            continue
        status = run['status']
        result = run['conclusion'] if status == 'completed' else status
        passed = passed and result == 'success' and status == 'completed'
        print(f'  {result or "unknown"} {run["url"]}')
    return passed


def main():
    print('Checking nightly CI status on main...')
    results = [check_workflow(workflow) for workflow in ('nightly.yml', 'fuzz.yml')]
    if all(results):
        print('\nNightly CI: all recent runs green.')
        return 0
    print('\nRelease blocked by nightly CI status.')
    return 1


if __name__ == '__main__':
    raise SystemExit(main())

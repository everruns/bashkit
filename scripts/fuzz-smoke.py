#!/usr/bin/env python3
"""Discover every fuzz target from Cargo.toml and run bounded PR smoke coverage."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
NIGHTLY = 'nightly-2026-10-30'


def matrix() -> list[dict]:
    manifest = tomllib.loads((ROOT / 'crates/bashkit/fuzz/Cargo.toml').read_text())
    rows = []
    for target in manifest['bin']:
        required = target.get('required-features', [])
        for enabled in (False, True):
            if 'plan_differential' in required and not enabled:
                continue
            features = required + (['experimental_execution_plan'] if enabled else [])
            rows.append({'target': target['name'], 'execution_plan': enabled, 'features': ','.join(features)})
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--matrix', action='store_true')
    parser.add_argument('--execution-plan', action='store_true')
    parser.add_argument('--target', help='Run one discovered target (default: all)')
    args = parser.parse_args()
    rows = matrix()
    if args.matrix:
        print(json.dumps({'include': rows}))
        return
    selected = [r for r in rows if r['execution_plan'] == args.execution_plan
                and (args.target is None or args.target == r['target'])]
    if not selected:
        parser.error('no targets selected')
    for row in selected:
        command = ['cargo', '+' + NIGHTLY, 'fuzz', 'run', '--codegen-units', '16']
        if row['features']:
            command += ['--features', row['features']]
        command += [row['target'], '--', '-runs=256', '-max_total_time=15',
                    '-max_len=512', '-timeout=10', '-print_final_stats=1']
        print(' '.join(command), flush=True)
        subprocess.run(command, cwd=ROOT / 'crates/bashkit', check=True)


if __name__ == '__main__':
    main()

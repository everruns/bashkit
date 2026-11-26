#!/usr/bin/env python3
"""Build both jq backends, measure alternating pairs, and save reviewable data.

Cargo's JSON artifacts locate executables across toolchains without depending
on private target/deps layouts. Build everything before measuring. Criterion
data and copied executables live in a private temporary directory.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def build(features: str, destination: Path, env: dict) -> None:
    command = ['cargo', 'bench', '--locked', '-p', 'bashkit', '--no-default-features',
               '--features', features, '--bench', 'jq', '--no-run', '--message-format=json']
    result = subprocess.run(command, cwd=ROOT, env=env, text=True,
                            stdout=subprocess.PIPE, check=True)
    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    binaries = [row['executable'] for row in artifacts
                if row.get('reason') == 'compiler-artifact'
                and row.get('target', {}).get('name') == 'jq' and row.get('executable')]
    if len(binaries) != 1:
        raise RuntimeError(f'expected one jq benchmark executable, found {binaries}')
    shutil.copy2(binaries[0], destination)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--quick', action='store_true', help='thin LTO/eight codegen units for iteration')
    parser.add_argument('--rounds', type=int, default=3)
    parser.add_argument('--samples', type=int, default=30)
    parser.add_argument('--warmup', type=float, default=0.5)
    parser.add_argument('--measurement', type=float, default=2.0)
    args = parser.parse_args()
    if args.rounds < 1 or args.samples < 10 or args.warmup <= 0 or args.measurement <= 0:
        parser.error('positive rounds/times and at least ten samples are required')
    env = os.environ.copy()
    if args.quick:
        env.update(CARGO_PROFILE_BENCH_LTO='thin', CARGO_PROFILE_BENCH_CODEGEN_UNITS='8')
    variants = {'established': 'jq', 'experimental': 'experimental_jq'}
    measurements = {name: {} for name in variants}
    cpu = None
    if hasattr(os, 'sched_getaffinity'):
        available = sorted(os.sched_getaffinity(0))
        cpu = available[len(available) // 2]
    with tempfile.TemporaryDirectory(prefix='bashkit-jq-') as scratch:
        scratch = Path(scratch)
        env['CRITERION_HOME'] = str(scratch / 'criterion')
        executables = {name: scratch / (name + ('.exe' if os.name == 'nt' else '')) for name in variants}
        for name, features in variants.items():
            build(features, executables[name], env)
        for round_index in range(args.rounds):
            order = list(variants) if round_index % 2 == 0 else list(reversed(variants))
            for name in order:
                label = f'{name}-{round_index}'
                command = [str(executables[name]), '--bench', '--noplot', '--sample-size', str(args.samples),
                           '--warm-up-time', str(args.warmup), '--measurement-time', str(args.measurement),
                           '--save-baseline', label]
                print(f'Round {round_index + 1}: {name}', flush=True)
                # Pin only the child; other validation work must be idle while
                # recording performance. Non-Linux native hosts run unpinned.
                with (scratch / f'{label}.log').open('w') as log:
                    subprocess.run(command, cwd=ROOT, env=env, stdout=log,
                                   stderr=subprocess.STDOUT, check=True,
                                   preexec_fn=(lambda: os.sched_setaffinity(0, {cpu})) if cpu is not None else None)
                for case in sorted((scratch / 'criterion/jq').iterdir()):
                    estimates = case / label / 'estimates.json'
                    if estimates.exists():
                        measurements[name].setdefault(case.name, []).append(json.loads(estimates.read_text()))
    if set(measurements['established']) != set(measurements['experimental']) or not measurements['established']:
        raise RuntimeError('both backends must report exactly the same nonempty benchmark set')
    rows = []
    for case in measurements['established']:
        before = measurements['established'][case]
        after = measurements['experimental'][case]
        if len(before) != args.rounds or len(after) != args.rounds:
            raise RuntimeError(f'{case}: incomplete paired rounds')
        before_ns = statistics.median(row['median']['point_estimate'] for row in before)
        after_ns = statistics.median(row['median']['point_estimate'] for row in after)
        rows.append({'benchmark': f'jq/{case}', 'established_median_ns': before_ns,
                     'experimental_median_ns': after_ns, 'change_pct': 100 * (after_ns / before_ns - 1),
                     'established_estimates': before, 'experimental_estimates': after})
    metadata = {'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
                'platform': f'{platform.system()} {platform.machine()}', 'cpu': cpu,
                'profile_overrides': {key: value for key, value in env.items() if key.startswith('CARGO_PROFILE_BENCH_')},
                'rounds': args.rounds, 'samples': args.samples, 'warmup_seconds': args.warmup,
                'measurement_seconds': args.measurement, 'features': variants, 'cases': rows}
    destination = ROOT / f'crates/bashkit/benches/results/criterion-jq-paired-{platform.system().lower()}-{platform.machine()}-{int(time.time())}'
    destination.with_suffix('.json').write_text(json.dumps(metadata, indent=2) + '\n')
    lines = ['# Criterion jq Paired Backend Benchmark', '',
             f"{metadata['rustc']}; {metadata['platform']}; CPU affinity: {cpu}.",
             f'{args.rounds} alternating rounds, {args.samples} samples, {args.warmup}s warmup, {args.measurement}s measurement target.',
             f"Bench profile overrides: `{metadata['profile_overrides']}`.",
             'Times are medians across round medians. Positive change means slower. Both backends assert identical output.', '',
             '| Benchmark | Before | After | Change |', '|---|---:|---:|---:|']
    for row in rows:
        lines.append(f"| `{row['benchmark']}` | {row['established_median_ns']/1e6:.3f} ms | {row['experimental_median_ns']/1e6:.3f} ms | {row['change_pct']:+.1f}% |")
    lines += ['', 'Before: `jq`; after: opt-in `experimental_jq`, both without default features.',
              'All builds finish before measurement. The paired JSON retains every round\'s Criterion estimates.',
              'Reproduce with `just bench-jq` (add `--quick` for thin-LTO development measurements).', '']
    destination.with_suffix('.md').write_text('\n'.join(lines))
    print(destination.relative_to(ROOT))
    subprocess.run(['node', 'site/scripts/build-performance-data.mjs'], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()

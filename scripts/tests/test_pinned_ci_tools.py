"""A source-built CI tool on the pinned toolchain must pin a version, in a
spelling the action actually reads.

`cache-cargo-install-action` runs `cargo install` against whatever toolchain
the job set up. Where that is the pinned one (rust-toolchain.toml), an unpinned
tool resolves to the newest release and an upstream MSRV bump breaks a tree
that has not changed: cargo-geiger 0.14.0 shipped on 2026-10-12 requiring rustc
1.97, and nightly run 259 went red the next morning with the unsafe-code audit
skipped. A job that installs the nightly toolchain is exempt — nightly outruns
any released MSRV — which is why `cargo-fuzz` in the fuzz job needs no pin.

The first attempt at pinning geiger passed `version: 0.13.0`. That action has
no `version` input, and GitHub Actions treats an unknown input as a warning
rather than an error, so the pin silently did nothing and run 260 installed
0.14.0 again with only this buried in the log:

    ##[warning]Unexpected input(s) 'version', valid inputs are
    ['tool', 'locked', 'git', 'tag', 'rev', 'features', ...]

Hence both checks below. The version has to be in `tool:` as `name@version`,
where the action reads it, and no step may pass an input the action drops. A
pin that looks right and does nothing is worse than no pin, because the job
goes on reporting the failure it was supposed to fix.
"""

from pathlib import Path
import re
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / '.github/workflows'

# The action builds from source, so its tool version rides the job's toolchain.
SOURCE_BUILD_ACTION = 'taiki-e/cache-cargo-install-action'
TOOLCHAIN_ACTION = 'dtolnay/rust-toolchain'

# Documented inputs, from the action's own error message. Anything else is
# accepted silently by the runner and ignored by the action.
VALID_INPUTS = {
    'tool',
    'locked',
    'git',
    'tag',
    'rev',
    'features',
    'no-default-features',
    'all-features',
}

# `dtolnay/rust-toolchain@<sha> # nightly`. The channel lives in the comment,
# which YAML drops, so collect those SHAs from the raw text. Dependabot moves
# the SHA and the comment together, so this keeps working across bumps.
NIGHTLY_REF = re.compile(
    rf'{re.escape(TOOLCHAIN_ACTION)}@([0-9a-f]{{40}})\s*#\s*nightly\b'
)


def nightly_toolchain_shas():
    shas = set()
    for path in WORKFLOWS.glob('*.yml'):
        shas.update(NIGHTLY_REF.findall(path.read_text()))
    return shas


def job_uses_nightly(job, nightly_shas):
    for step in job.get('steps') or []:
        uses = step.get('uses') or ''
        if TOOLCHAIN_ACTION in uses:
            _, _, ref = uses.partition('@')
            if ref in nightly_shas:
                return True
    return False


def source_build_steps():
    """(workflow, job, step, on_nightly) per source-build action step."""
    nightly_shas = nightly_toolchain_shas()
    for path in sorted(WORKFLOWS.glob('*.yml')):
        spec = yaml.safe_load(path.read_text())
        for job_name, job in (spec.get('jobs') or {}).items():
            on_nightly = job_uses_nightly(job, nightly_shas)
            for step in job.get('steps') or []:
                if SOURCE_BUILD_ACTION in (step.get('uses') or ''):
                    yield path.name, job_name, step, on_nightly


class PinnedCiToolTests(unittest.TestCase):
    def test_discovery_finds_both_kinds_of_job(self):
        # A selector that matched nothing, or that lost the nightly
        # distinction, would make the checks below vacuous.
        steps = list(source_build_steps())
        self.assertTrue(steps, f'no {SOURCE_BUILD_ACTION} steps found')
        self.assertTrue(
            nightly_toolchain_shas(),
            f'no `{TOOLCHAIN_ACTION}@<sha> # nightly` ref found; the comment '
            'spelling this parser relies on may have changed',
        )

    def test_tools_on_the_pinned_toolchain_pin_a_version(self):
        for workflow, job, step, on_nightly in source_build_steps():
            if on_nightly:
                continue
            tool = (step.get('with') or {}).get('tool', '')
            with self.subTest(workflow=workflow, job=job, tool=tool):
                name, at, version = tool.partition('@')
                self.assertTrue(
                    at and name and version,
                    f'{workflow}:{job} builds {tool!r} from source on the '
                    'pinned toolchain with no version; pin it as '
                    '`tool: name@version` so an upstream MSRV bump cannot '
                    'break a tree that has not changed',
                )

    def test_no_step_passes_an_input_the_action_ignores(self):
        # The failure mode this exists for: `version:` here is dropped with a
        # warning, so the pin reads as correct and does nothing.
        for workflow, job, step, _ in source_build_steps():
            for key in step.get('with') or {}:
                with self.subTest(workflow=workflow, job=job, input=key):
                    self.assertIn(
                        key,
                        VALID_INPUTS,
                        f'{workflow}:{job} passes {key!r} to '
                        f'{SOURCE_BUILD_ACTION}, which does not accept it; '
                        'the runner only warns, so it is silently ignored',
                    )


if __name__ == '__main__':
    unittest.main()

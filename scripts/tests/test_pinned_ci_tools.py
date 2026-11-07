"""A source-built CI tool on the pinned toolchain must pin a version, in a
spelling the action actually reads.

`cache-cargo-install-action` runs `cargo install` against whatever toolchain
the job set up. Where that is the pinned one (rust-toolchain.toml), an unpinned
tool resolves to the newest release and an upstream MSRV bump breaks a tree
that has not changed: cargo-geiger 0.14.0 shipped on 2026-10-12 requiring rustc
1.97, and nightly run 259 went red the next morning with the unsafe-code audit
skipped. A step that *runs on* a pinned nightly is exempt — nightly outruns any
released MSRV — which is why `cargo-fuzz` needs no pin.

"Runs on nightly" is a property of the step, not of the job. The toolchain
action dropped its `toolchain:` input, so a job that wants nightly now installs
it alongside a stable default (`rustup toolchain install nightly-...`) and
selects it per command (`cargo +nightly-... fuzz build`). A `cargo install` in
such a job gets the *stable* default unless it opts in, so merely containing a
nightly install no longer exempts a step: it must name the nightly itself, via
`RUSTUP_TOOLCHAIN` on the step or `cargo +nightly-...` in its `run`, either
of which may name the nightly through `${{ env.X }}`. Exempting
per job here would have silently reopened the geiger hole for `cargo-fuzz`.

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


# The toolchain action removed its `toolchain:` input, so newer jobs install
# the pinned nightly with an explicit run step instead of a `# nightly`-marked
# action ref. Either spelling marks the job as *having* a nightly available.
RUSTUP_NIGHTLY_INSTALL = re.compile(r'rustup\s+toolchain\s+install\s+\S*nightly')

# `cargo +nightly-2026-10-30 install ...` — the per-command toolchain selector.
CARGO_PLUS_NIGHTLY = re.compile(r'cargo\s+\+\S*nightly')
# `${{ env.RUST_NIGHTLY }}` — a job naming its nightly through a variable.
ENV_EXPR = re.compile(r'\$\{\{\s*env\.([A-Za-z_][A-Za-z0-9_]*)\s*\}\}')


def env_scopes(spec, job, step=None):
    """`env` mappings that apply to a step, innermost first."""
    return [
        (step or {}).get('env') or {},
        job.get('env') or {},
        spec.get('env') or {},
    ]


def resolve_env(value, scopes):
    """`value` with `${{ env.NAME }}` replaced from the first scope defining NAME.

    An unresolved reference is left as written, which keeps `_is_nightly`
    false rather than matching the variable's own name: `RUST_NIGHTLY` is not
    evidence that what it holds is a nightly.
    """
    def sub(match):
        name = match.group(1)
        for scope in scopes:
            if name in scope:
                return str(scope[name])
        return match.group(0)

    return ENV_EXPR.sub(sub, value or '')


def _is_nightly(value, scopes=()):
    return 'nightly' in resolve_env(value, scopes)


def job_installs_nightly_via_rustup(spec, job):
    for step in job.get('steps') or []:
        run = resolve_env(step.get('run') or '', env_scopes(spec, job, step))
        if RUSTUP_NIGHTLY_INSTALL.search(run):
            return True
    return False


def job_default_is_nightly(job, nightly_shas):
    """True when the job's *default* toolchain is a nightly.

    Only the old `dtolnay/rust-toolchain@<sha> # nightly` ref does this. The
    rustup spelling installs nightly next to the stable default, so commands
    there get stable unless they ask for nightly.
    """
    for step in job.get('steps') or []:
        uses = step.get('uses') or ''
        if TOOLCHAIN_ACTION in uses:
            _, _, ref = uses.partition('@')
            if ref in nightly_shas:
                return True
    return False


def job_has_nightly(spec, job, nightly_shas):
    """True when the job sets up a nightly at all, in either spelling."""
    return job_default_is_nightly(job, nightly_shas) or (
        job_installs_nightly_via_rustup(spec, job)
    )


def step_runs_on_nightly(spec, job, step, nightly_shas):
    """True when this step's `cargo` resolves to a nightly toolchain."""
    if job_default_is_nightly(job, nightly_shas):
        return True
    # Opt-in, in the two spellings that actually redirect cargo. Both may name
    # the nightly through `${{ env.X }}`, which is how the Pyodide jobs do it.
    scopes = env_scopes(spec, job, step)
    env = {**(job.get('env') or {}), **(step.get('env') or {})}
    if _is_nightly(env.get('RUSTUP_TOOLCHAIN'), scopes):
        return True
    run = resolve_env(step.get('run') or '', scopes)
    return bool(CARGO_PLUS_NIGHTLY.search(run))


def nightly_job_ids():
    """`workflow:job` ids setting up a nightly toolchain, either spelling."""
    shas = nightly_toolchain_shas()
    ids = set()
    for path in sorted(WORKFLOWS.glob('*.yml')):
        spec = yaml.safe_load(path.read_text())
        for job_name, job in (spec.get('jobs') or {}).items():
            if job_has_nightly(spec, job, shas):
                ids.add(f'{path.name}:{job_name}')
    return ids


def source_build_steps():
    """(workflow, job, step, on_nightly) per source-build action step."""
    nightly_shas = nightly_toolchain_shas()
    for path in sorted(WORKFLOWS.glob('*.yml')):
        spec = yaml.safe_load(path.read_text())
        for job_name, job in (spec.get('jobs') or {}).items():
            for step in job.get('steps') or []:
                if SOURCE_BUILD_ACTION in (step.get('uses') or ''):
                    yield (
                        path.name,
                        job_name,
                        step,
                        step_runs_on_nightly(spec, job, step, nightly_shas),
                    )


# `cargo install <tool>` in a `run:` block builds from source just like the
# action does, so it carries the same MSRV hazard and needs the same pin.
CARGO_INSTALL = re.compile(
    r'cargo\s+(?:\+\S+\s+)?install\s+(?P<rest>[^\n]*)'
)


def cargo_install_run_steps():
    """(workflow, job, tool, pinned, on_nightly) per `cargo install` run step."""
    nightly_shas = nightly_toolchain_shas()
    for path in sorted(WORKFLOWS.glob('*.yml')):
        spec = yaml.safe_load(path.read_text())
        for job_name, job in (spec.get('jobs') or {}).items():
            for step in job.get('steps') or []:
                run = step.get('run') or ''
                for match in CARGO_INSTALL.finditer(run):
                    rest = match.group('rest').split()
                    tools = [arg for arg in rest if not arg.startswith('-')]
                    if not tools:
                        continue
                    tool = tools[0]
                    pinned = '--version' in rest or '@' in tool
                    yield (
                        path.name,
                        job_name,
                        tool,
                        pinned,
                        step_runs_on_nightly(spec, job, step, nightly_shas),
                    )


class PinnedCiToolTests(unittest.TestCase):
    def test_discovery_finds_both_kinds_of_job(self):
        # A selector that matched nothing, or that lost the nightly
        # distinction, would make the checks below vacuous.
        steps = list(source_build_steps())
        self.assertTrue(steps, f'no {SOURCE_BUILD_ACTION} steps found')
        self.assertTrue(
            nightly_job_ids(),
            f'no nightly toolchain install found (neither a '
            f'`{TOOLCHAIN_ACTION}@<sha> # nightly` ref nor a '
            '`rustup toolchain install nightly-...` step); the spelling '
            'this parser relies on may have changed',
        )
        self.assertTrue(
            list(cargo_install_run_steps()),
            'no `cargo install` run steps found; the spelling this parser '
            'relies on may have changed',
        )
        # The exemption is worth nothing if nothing is ever exempt.
        on_nightly = [
            True for *_, nightly in source_build_steps() if nightly
        ] + [True for *_, nightly in cargo_install_run_steps() if nightly]
        self.assertTrue(
            on_nightly,
            'no source-built tool runs on nightly; the opt-in spellings '
            '(`RUSTUP_TOOLCHAIN`, `cargo +nightly-...`) may have changed',
        )

    def test_cargo_install_on_the_pinned_toolchain_pins_a_version(self):
        # Same hazard as the action, different spelling: an unpinned
        # `cargo install` on the pinned stable resolves to the newest release.
        for workflow, job, tool, pinned, on_nightly in cargo_install_run_steps():
            if on_nightly:
                continue
            with self.subTest(workflow=workflow, job=job, tool=tool):
                self.assertTrue(
                    pinned,
                    f'{workflow}:{job} runs `cargo install {tool}` on the '
                    'pinned toolchain with no version; pass `--version <v>` '
                    'or run it on the nightly (`cargo +nightly-... install`) '
                    'so an upstream MSRV bump cannot break a tree that has '
                    'not changed',
                )

    def test_the_detector_follows_an_env_indirection(self):
        # A job can name its nightly through a variable: the Pyodide jobs run
        # `rustup toolchain install ${{ env.RUST_NIGHTLY }}` and set
        # `RUSTUP_TOOLCHAIN` to the same expression — the very route this
        # file treats as an opt-in. Matching only a literal `nightly` token
        # called those jobs non-nightly, so a source build added there would
        # be asked to pin a tool that is already on a nightly. Every job that
        # installs a toolchain through rustup must be discovered.
        shas = nightly_toolchain_shas()
        checked = 0
        for path in sorted(WORKFLOWS.glob('*.yml')):
            spec = yaml.safe_load(path.read_text())
            for job_name, job in (spec.get('jobs') or {}).items():
                installs = [
                    step for step in job.get('steps') or []
                    if 'rustup toolchain install' in (step.get('run') or '')
                ]
                if not installs:
                    continue
                checked += 1
                with self.subTest(workflow=path.name, job=job_name):
                    self.assertTrue(
                        job_has_nightly(spec, job, shas),
                        f'{path.name}:{job_name} installs a toolchain via rustup '
                        'but the detector does not see a nightly',
                    )
                    for step in installs:
                        self.assertTrue(
                            step_runs_on_nightly(spec, job, step, shas)
                            or not (step.get('env') or {}).get('RUSTUP_TOOLCHAIN'),
                            'a step pinning RUSTUP_TOOLCHAIN to this job\'s nightly '
                            'must count as running on it',
                        )
        self.assertGreaterEqual(checked, 4, 'rustup install jobs not found')

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

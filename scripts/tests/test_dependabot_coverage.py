"""Every dependency-resolving workspace in the tree needs a Dependabot entry.

Dependabot cannot glob: each workspace has to be named in
`.github/dependabot.yml` by hand, and that list has to grow with every new
workspace. This is the same shape of manual step that failed twice for the
advisory scan (see the 2026-08-22 and 2026-10-02 entries in knowledge/log.md),
which is why that scan discovers lockfiles instead of listing them. Dependabot
has no discovery to switch to, so the list is checked instead.

It had already drifted when this test was written:
`crates/bashkit-python/test-fixtures/random-fs` is an independent cargo
workspace with a `pyo3` dependency and had no entry, while its napi twin
`crates/bashkit-js/test-fixtures/random-fs` did. Both fixtures' lockfiles were
gitignored then (committed since 2026-11-06), so the advisory scan did not see
them in a fresh checkout either — Dependabot is their only coverage, and for one of
them there was none.

A missing entry is quiet by nature: nothing fails, no PR is opened, and the
dependency simply stops being watched. That is what this test converts into a
loud failure.
"""

from pathlib import Path
import tomllib
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
CONFIG = ROOT / '.github/dependabot.yml'
SKIP = ('target', 'node_modules', '.git', '.venv')


def tracked_dirs(pattern, predicate=None):
    """Directories under ROOT holding `pattern`, excluding build/dependency trees."""
    found = set()
    for path in ROOT.rglob(pattern):
        if any(part in SKIP for part in path.relative_to(ROOT).parts):
            continue
        if predicate and not predicate(path):
            continue
        rel = path.parent.relative_to(ROOT).as_posix()
        found.add('/' if rel == '.' else f'/{rel}')
    return found


def declares_workspace(manifest):
    """True for a Cargo.toml that resolves its own dependency graph.

    A `[workspace]` table marks either the root workspace or a package
    deliberately detached from it; both get their own lockfile, so the parent's
    Dependabot entry does not reach them.
    """
    return any(
        line.strip() == '[workspace]'
        for line in manifest.read_text().splitlines()
    )


def configured_dirs(ecosystem):
    config = yaml.safe_load(CONFIG.read_text())
    covered = set()
    for update in config['updates']:
        if update['package-ecosystem'] != ecosystem:
            continue
        if 'directory' in update:
            covered.add(update['directory'])
        covered.update(update.get('directories') or [])
    return covered


class DependabotCoverageTests(unittest.TestCase):
    def assert_covered(self, ecosystem, expected):
        covered = configured_dirs(ecosystem)
        missing = expected - covered
        self.assertFalse(
            missing,
            f'{ecosystem} workspaces with no dependabot.yml entry: '
            f'{sorted(missing)} (dependabot cannot glob; add each one)',
        )

    def test_every_cargo_workspace_is_covered(self):
        workspaces = tracked_dirs('Cargo.toml', declares_workspace)
        # Guard the discovery itself: a predicate that silently matched nothing
        # would make this test pass by finding no work to do.
        self.assertGreaterEqual(len(workspaces), 6, workspaces)
        self.assertIn('/', workspaces)
        self.assert_covered('cargo', workspaces)

    def test_every_pnpm_project_is_covered(self):
        projects = tracked_dirs('pnpm-lock.yaml')
        self.assertGreaterEqual(len(projects), 5, projects)
        self.assert_covered('npm', projects)

    def test_every_uv_project_is_covered(self):
        projects = tracked_dirs('uv.lock')
        self.assertGreaterEqual(len(projects), 1, projects)
        self.assert_covered('uv', projects)

    def test_github_actions_is_covered(self):
        self.assertIn('/', configured_dirs('github-actions'))

    def test_the_two_random_fs_fixtures_are_both_covered(self):
        # The specific drift this test was written for. Both are independent
        # workspaces with third-party dependencies (lockfiles committed since 2026-11-06), so
        # Dependabot is the only thing watching either one.
        covered = configured_dirs('cargo')
        for fixture in (
            '/crates/bashkit-js/test-fixtures/random-fs',
            '/crates/bashkit-python/test-fixtures/random-fs',
        ):
            with self.subTest(fixture=fixture):
                self.assertIn(fixture, covered)

    def test_no_cargo_entry_can_propose_a_wasmtime_bump(self):
        # wasmtime is held at =48.0.6 for two independent reasons: the
        # `cpython` feature loads a module precompiled by build.rs, which only
        # loads into the identical wasmtime version, and 48.x is the newest
        # line whose MSRV the pinned rustc satisfies. 49.x needs 1.96.
        #
        # The "/" entry carried that ignore; the fuzz and example entries did
        # not, and the ignore is per-entry. The fuzz lockfile resolves the root
        # workspace, so dependabot edited the root `=48.0.6` pin straight
        # through the gap and opened #2708, where every rust job died at
        # resolution with "wasmtime@49.0.2 requires rustc 1.96.0". The
        # hyperlight host pins its own wasmtime "48" and is built by required
        # CI on the same pinned toolchain, so it had the same exposure.
        #
        # Which entry happens to reach which manifest is not a useful thing to
        # re-derive each time one is added, so the invariant is the blunt one:
        # no cargo entry proposes a wasmtime bump. Lift it everywhere at once
        # when the toolchain moves.
        config = yaml.safe_load(CONFIG.read_text())
        cargo_entries = [
            u for u in config['updates'] if u['package-ecosystem'] == 'cargo'
        ]
        self.assertGreaterEqual(len(cargo_entries), 3, cargo_entries)
        for entry in cargo_entries:
            scope = entry.get('directory') or entry.get('directories')
            with self.subTest(scope=scope):
                ignored = {
                    i.get('dependency-name')
                    for i in entry.get('ignore') or []
                    # An ignore narrowed to update-types still lets other
                    # bump kinds through, so it does not count here.
                    if not i.get('update-types')
                }
                self.assertIn(
                    'wasmtime',
                    ignored,
                    f'cargo entry {scope} can propose a wasmtime bump; '
                    'it is pinned to =48.0.6 by MSRV and by precompiled-module '
                    'compatibility, so every cargo entry must ignore it',
                )

    def test_entries_that_path_depend_on_bashkit_allow_only_own_deps(self):
        # A manifest that path-depends on crates/bashkit makes Dependabot walk
        # into the root workspace and treat every `workspace = true` dep of
        # bashkit as this directory's own. It cannot change those (the root
        # manifest holds them) and the optional ones are in no lockfile here,
        # so the grouped job aborted with InvalidUpdatedDependencies every week
        # (2026-10-13 .. 2026-11-06). The fix is an `allow` list; this keeps it
        # complete, since a dep missing from it is silently never updated.
        config = yaml.safe_load(CONFIG.read_text())
        checked = 0
        for entry in config['updates']:
            if entry['package-ecosystem'] != 'cargo':
                continue
            dirs = entry.get('directories') or [entry.get('directory')]
            for d in dirs:
                manifest = ROOT / d.lstrip('/') / 'Cargo.toml'
                data = tomllib.loads(manifest.read_text())
                deps = {}
                for table in ('dependencies', 'build-dependencies', 'dev-dependencies'):
                    deps.update(data.get(table) or {})
                if not any(
                    isinstance(v, dict) and 'path' in v and 'bashkit' in v['path']
                    for v in deps.values()
                ):
                    continue
                checked += 1
                allowed = {a.get('dependency-name') for a in entry.get('allow') or []}
                own = {
                    name for name, v in deps.items()
                    if not (isinstance(v, dict) and 'path' in v)
                }
                with self.subTest(directory=d):
                    self.assertTrue(allowed, f'{d} path-depends on bashkit but its entry has no allow list')
                    self.assertFalse(own - allowed, f'{d}: direct deps missing from allow: {sorted(own - allowed)}')
        # hyperlight guest + the two random-fs fixtures (the fuzz crate uses
        # `path = ".."` and its job is green, so it is not matched).
        self.assertGreaterEqual(checked, 3)

    def test_root_requirements_are_not_prereleases_over_a_stable_lock(self):
        # `turso_core = "0.8.0-pre.1"` with 0.8.3 locked made Dependabot think
        # 0.9.0-pre.2 was in range; its lockfile-only update then failed in
        # cargo (`^0.8.0-pre.1` stops below 0.9) and reddened the weekly "/"
        # job from 2026-10-19 on. Require the stable version once one is locked.
        workspace = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']
        lock = tomllib.loads((ROOT / 'Cargo.lock').read_text())
        locked = {}
        for pkg in lock['package']:
            locked.setdefault(pkg['name'], []).append(pkg['version'])
        for name, spec in workspace['dependencies'].items():
            req = spec if isinstance(spec, str) else spec.get('version')
            if not req or '-' not in req:
                continue
            versions = locked.get(spec.get('package', name) if isinstance(spec, dict) else name, [])
            with self.subTest(dependency=name):
                self.assertFalse(
                    any('-' not in v for v in versions),
                    f'{name} requires prerelease {req!r} but the lock holds {versions}; '
                    'use the stable version so Dependabot does not offer out-of-range prereleases',
                )

    def test_fork_only_deps_are_ignored_by_the_root_entry(self):
        # crates/bashkit-monty(-types) are forks of Monty 1.4.0 and keep its
        # dependency versions until upstream source is re-copied. Crates only
        # they declare must not be bumped by Dependabot (#2848 did, and broke
        # the fork build). Shared ones stay bumpable for bashkit's sake.
        def direct(manifest):
            data = tomllib.loads(manifest.read_text())
            names = set()
            for table in ('dependencies', 'build-dependencies', 'dev-dependencies'):
                for name, spec in (data.get(table) or {}).items():
                    if isinstance(spec, dict) and 'path' in spec:
                        continue
                    if isinstance(spec, dict):
                        name = spec.get('package', name)
                    names.add(name)
            return names

        forks = ('bashkit-monty', 'bashkit-monty-types')
        fork_deps = set().union(*(direct(ROOT / 'crates' / f / 'Cargo.toml') for f in forks))
        other = set()
        for manifest in (ROOT / 'crates').glob('*/Cargo.toml'):
            if manifest.parent.name not in forks:
                other |= direct(manifest)
        fork_only = fork_deps - other
        self.assertGreaterEqual(len(fork_only), 10, fork_only)

        config = yaml.safe_load(CONFIG.read_text())
        root = next(
            u for u in config['updates']
            if u['package-ecosystem'] == 'cargo' and u.get('directory') == '/'
        )
        ignored = {
            i.get('dependency-name') for i in root.get('ignore') or []
            if not i.get('update-types')
        }
        self.assertFalse(
            fork_only - ignored,
            f'fork-only deps Dependabot may bump: {sorted(fork_only - ignored)}',
        )

    def test_path_dep_workspaces_commit_their_lockfile(self):
        # Without a lockfile Dependabot has no previous version for an in-range
        # bump and aborts the whole grouped job; cargo has no
        # `versioning-strategy` that works around it.
        for d in (
            'crates/bashkit-js/test-fixtures/random-fs',
            'crates/bashkit-python/test-fixtures/random-fs',
            'examples/hyperlight',
            'examples/hyperlight/host',
        ):
            with self.subTest(directory=d):
                self.assertTrue((ROOT / d / 'Cargo.lock').is_file(), d)

    def test_path_dep_lockfiles_pin_the_workspace_bashkit_version(self):
        # Builds are not `--locked`, so a lockfile left on the old version after
        # a release bump is silently refreshed in CI while Dependabot and the
        # advisory scan keep reading the stale one. Fail loudly instead; the
        # refresh loop is in knowledge/operations/release-process.md.
        version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
        checked = 0
        for d in (
            'crates/bashkit/fuzz',
            'crates/bashkit-js/test-fixtures/random-fs',
            'crates/bashkit-python/test-fixtures/random-fs',
            'examples/hyperlight',
        ):
            lock = tomllib.loads((ROOT / d / 'Cargo.lock').read_text())
            pinned = [
                p['version'] for p in lock['package']
                if p['name'] == 'bashkit' and 'source' not in p
            ]
            with self.subTest(directory=d):
                self.assertEqual(pinned, [version], f'{d}/Cargo.lock is stale; refresh it')
                checked += 1
        self.assertEqual(checked, 4)

    def test_cargo_versioning_strategy_is_one_cargo_accepts(self):
        # #2852 set `increase` on a cargo entry; Dependabot rejected the whole
        # file ("did not match one of: lockfile-only, auto"), which stops every
        # ecosystem's updates, not just that entry's.
        config = yaml.safe_load(CONFIG.read_text())
        for update in config['updates']:
            if update['package-ecosystem'] != 'cargo':
                continue
            strategy = update.get('versioning-strategy', 'auto')
            with self.subTest(scope=update.get('directory') or update.get('directories')):
                self.assertIn(strategy, {'auto', 'lockfile-only'})

    def test_config_is_a_well_formed_dependabot_v2_file(self):
        config = yaml.safe_load(CONFIG.read_text())
        self.assertEqual(config['version'], 2)
        for update in config['updates']:
            self.assertIn('package-ecosystem', update)
            self.assertIn('schedule', update)
            # Exactly one of the two spellings; both at once is ambiguous and
            # neither means the entry covers nothing.
            self.assertEqual(
                1,
                ('directory' in update) + ('directories' in update),
                update.get('package-ecosystem'),
            )


if __name__ == '__main__':
    unittest.main()

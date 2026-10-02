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
`crates/bashkit-js/test-fixtures/random-fs` did. Both fixtures' lockfiles are
gitignored and generated at test time, so the advisory scan does not see them
in a fresh checkout either — Dependabot is their only coverage, and for one of
them there was none.

A missing entry is quiet by nature: nothing fails, no PR is opened, and the
dependency simply stops being watched. That is what this test converts into a
loud failure.
"""

from pathlib import Path
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
        # workspaces with third-party dependencies and gitignored lockfiles, so
        # Dependabot is the only thing watching either one.
        covered = configured_dirs('cargo')
        for fixture in (
            '/crates/bashkit-js/test-fixtures/random-fs',
            '/crates/bashkit-python/test-fixtures/random-fs',
        ):
            with self.subTest(fixture=fixture):
                self.assertIn(fixture, covered)

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

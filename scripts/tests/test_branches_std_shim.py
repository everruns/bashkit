"""The `branches` feature-unification shim, and the ways it can silently lapse.

`turso_core` depends on `branches` with `default-features = false`. The
resulting no_std body of `branches::abort()` is `core::intrinsics::abort()`, an
unstable intrinsic rustc removed in 1.101.0-nightly, so the crate stopped
compiling there — nightly run #242 went red on an unchanged tree while stable
CI stayed green. `branches` has no fixed release (0.5.0 carries the same line)
and Cargo cannot add a feature to a transitive dependency, so `bashkit`
declares a direct edge under the `sqlite` feature purely to turn `std` on for
the whole graph.

Every part of that is quiet when it breaks. Cargo unifies features per
*version*, so the shim stops working the moment a second copy of `branches`
resolves — and nothing in a stable build notices, because only the nightly
ASan job both tracks floating nightly and builds `turso_core`. The failure
reappears a day later, in a job nobody is watching, on a tree that looks fine.
These tests convert each way that can happen into a loud failure.

See knowledge/operations/dependencies.md#feature-unification-edges.
"""
from pathlib import Path
import tomllib
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]


def load(path):
    with open(ROOT / path, 'rb') as handle:
        return tomllib.load(handle)


class BranchesStdShimTests(unittest.TestCase):
    def test_workspace_declares_branches_with_std_and_nothing_else(self):
        # `std` is what replaces the unstable intrinsic with
        # std::process::abort(). default-features would also drag in
        # `prefetch`, which this shim has no reason to enable.
        dep = load('Cargo.toml')['workspace']['dependencies']['branches']
        self.assertEqual(dep['features'], ['std'])
        self.assertIs(dep['default-features'], False)

    def test_shim_stays_on_the_version_line_turso_core_resolves(self):
        # Cargo unifies features per version. turso_core requires `^0.4.3`, so
        # a declaration of 0.5.0 here resolves a *second* copy and leaves
        # turso_core's 0.4.x one with no `std` — the original breakage, back
        # with the shim still present and looking correct.
        req = load('Cargo.toml')['workspace']['dependencies']['branches']['version']
        self.assertTrue(
            req.startswith('0.4.'),
            f'branches must stay on the 0.4 line turso_core resolves, got {req!r}',
        )

    def test_exactly_one_copy_of_branches_is_locked(self):
        # The invariant the two tests above exist to protect, asserted against
        # what Cargo actually resolved. Two copies means the shim applies to
        # one of them and turso_core compiles the other without `std`.
        versions = [
            pkg['version']
            for pkg in load('Cargo.lock')['package']
            if pkg['name'] == 'branches'
        ]
        self.assertEqual(
            len(versions), 1, f'expected one locked `branches`, got {versions}'
        )

    def test_sqlite_feature_activates_the_shim(self):
        # The edge has to be active exactly when turso_core is. An optional dep
        # that no feature enables is a no-op that still reads as a fix.
        manifest = load('crates/bashkit/Cargo.toml')
        self.assertTrue(manifest['dependencies']['branches']['optional'])
        sqlite = manifest['features']['sqlite']
        self.assertIn('dep:branches', sqlite)
        self.assertIn('dep:turso_core', sqlite)

    def test_dependabot_ignores_branches(self):
        # Dependabot rewrites manifests, and a bump to 0.5.0 is exactly the
        # second-copy case above. 0.5.0 carries the same unstable intrinsic, so
        # there is nothing a bump could gain.
        config = yaml.safe_load((ROOT / '.github/dependabot.yml').read_text())
        root_cargo = next(
            entry
            for entry in config['updates']
            if entry['package-ecosystem'] == 'cargo' and entry['directory'] == '/'
        )
        ignored = {item['dependency-name'] for item in root_cargo['ignore']}
        self.assertIn('branches', ignored)


if __name__ == '__main__':
    unittest.main()

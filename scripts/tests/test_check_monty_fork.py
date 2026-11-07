"""Tests for scripts/check_monty_fork.py (offline, synthetic upstream trees)."""

from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_monty_fork as cmf  # noqa: E402

UPSTREAM_SRC = 'fn a() {}\nfn b() {\n    1\n}\nfn c() {}\n'
# cargo-normalized upstream manifest vs the fork's hand-written one.
UPSTREAM_TOML = """[package]
name = "monty"

[features]
hooks = ["monty-types/hooks"]

[dependencies.monty-types]
version = "1.4.0"

[dependencies.serde]
version = "1.0"
features = ["derive"]

[dev-dependencies.insta]
version = "1"
"""
FORK_TOML = """[package]
name = "bashkit-monty"

[dependencies]
monty-types = { package = "bashkit-monty-types", path = "../t", version = "0.20.3" }
serde = { version = "1.0", features = ["derive"] }

[features]
hooks = ["monty-types/hooks"]
"""


class CheckMontyForkTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.up = base / 'up'
        self.fork = base / 'fork'
        for d in (self.up, self.fork):
            (d / 'src').mkdir(parents=True)
            (d / 'src' / 'lib.rs').write_text(UPSTREAM_SRC)
        (self.up / 'Cargo.toml').write_text(UPSTREAM_TOML)
        (self.fork / 'Cargo.toml').write_text(FORK_TOML)

    def tearDown(self):
        self.tmp.cleanup()

    def write_fork(self, text):
        (self.fork / 'src' / 'lib.rs').write_text(text)

    def test_identical_fork_passes(self):
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_marked_change_passes(self):
        self.write_fork(UPSTREAM_SRC.replace('    1\n', '    // bashkit fork: why\n    2\n'))
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_marker_just_above_change_passes(self):
        self.write_fork(UPSTREAM_SRC.replace('fn b() {\n    1\n', '// bashkit fork: why\nfn b() {\n    2\n'))
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_unmarked_change_fails(self):
        self.write_fork(UPSTREAM_SRC.replace('    1\n', '    2\n'))
        errors = cmf.check_crate(self.fork, self.up)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn('src/lib.rs:3', errors[0])

    def test_unmarked_deletion_fails(self):
        self.write_fork(UPSTREAM_SRC.replace('fn c() {}\n', ''))
        self.assertEqual(len(cmf.check_crate(self.fork, self.up)), 1)

    # Upstream item whose doc comment and `#[inline]` stay unchanged in the
    # fork, so a fork note inserted into the doc block and a change to the
    # signature below it are two separate diff hunks (as in `check_time_every`).
    DOCUMENTED = (
        'fn a() {}\n'
        '/// doc one\n'
        '/// doc two\n'
        '#[inline]\n'
        'fn b() {\n'
        '    1\n'
        '}\n'
    )
    NOTE = (
        '/// bashkit fork: first line carries the marker\n'
        '/// second line of the rationale\n'
        '/// third line of the rationale\n'
        '/// fourth line of the rationale\n'
    )

    def write_pair(self, upstream, fork):
        (self.up / 'src' / 'lib.rs').write_text(upstream)
        self.write_fork(fork)

    def test_marker_at_the_top_of_a_long_note_covers_the_item(self):
        """The marker is on the note's first line, beyond MARKER_WINDOW."""
        self.write_pair(
            self.DOCUMENTED,
            self.DOCUMENTED.replace('#[inline]\n', self.NOTE + '#[inline]\n').replace(
                'fn b() {', 'fn b(x: u8) {'
            ),
        )
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_unmarked_long_note_still_fails(self):
        note = self.NOTE.replace('bashkit fork: first', 'first')
        self.write_pair(
            self.DOCUMENTED,
            self.DOCUMENTED.replace('#[inline]\n', note + '#[inline]\n').replace(
                'fn b() {', 'fn b(x: u8) {'
            ),
        )
        self.assertTrue(cmf.check_crate(self.fork, self.up))

    def test_comment_block_does_not_reach_past_code(self):
        """A note on one item cannot mark drift in the item after it."""
        self.write_pair(
            self.DOCUMENTED + 'fn c() {}\n',
            self.DOCUMENTED.replace('/// doc one', '// bashkit fork: why\n/// doc one')
            + 'fn c() { 0 }\n',
        )
        self.assertTrue(cmf.check_crate(self.fork, self.up))

    def test_marker_far_above_does_not_cover_change(self):
        text = '// bashkit fork: unrelated\n' + UPSTREAM_SRC.replace('fn c() {}', 'fn c() { 0 }')
        self.write_fork(text)
        self.assertEqual(len(cmf.check_crate(self.fork, self.up)), 1)

    def test_fork_only_file_needs_marker_header(self):
        (self.fork / 'src' / 'extra.rs').write_text('fn x() {}\n')
        self.assertEqual(len(cmf.check_crate(self.fork, self.up)), 1)
        (self.fork / 'src' / 'extra.rs').write_text('// bashkit fork: new hook\nfn x() {}\n')
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_packaging_files_are_ignored(self):
        (self.fork / 'LICENSE').write_text('MIT\n')
        (self.fork / 'rustfmt.toml').write_text('disable_all_formatting = true\n')
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_upstream_tests_and_packaging_may_be_dropped(self):
        for rel in ('tests/t.rs', 'test_cases/x.py', 'Cargo.lock', 'Cargo.toml.orig', '.cargo_vcs_info.json'):
            (self.up / rel).parent.mkdir(parents=True, exist_ok=True)
            (self.up / rel).write_text('x\n')
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_deleted_upstream_file_fails(self):
        (self.up / 'src' / 'gone.rs').write_text('fn g() {}\n')
        errors = cmf.check_crate(self.fork, self.up)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn('src/gone.rs', errors[0])

    def test_binary_files_compare_as_bytes(self):
        (self.up / 'src' / 'blob.bin').write_bytes(b'\xff\x00')
        (self.fork / 'src' / 'blob.bin').write_bytes(b'\xff\x00')
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])
        (self.fork / 'src' / 'blob.bin').write_bytes(b'\xff\x01')
        errors = cmf.check_crate(self.fork, self.up)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn('binary', errors[0])

    def assert_manifest_error(self, fork_toml, needle):
        (self.fork / 'Cargo.toml').write_text(fork_toml)
        errors = cmf.check_crate(self.fork, self.up)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn(needle, errors[0])

    def test_added_dependency_fails(self):
        self.assert_manifest_error(FORK_TOML.replace('[features]', 'evil = "1"\n\n[features]'), 'adds `evil`')

    def test_dropped_dependency_fails(self):
        self.assert_manifest_error(FORK_TOML.replace('serde = { version = "1.0", features = ["derive"] }\n', ''), 'drops upstream `serde`')

    def test_changed_dependency_features_fail(self):
        self.assert_manifest_error(FORK_TOML.replace('["derive"]', '["derive", "rc"]'), '`serde` differs')

    def test_changed_dependency_version_fails(self):
        self.assert_manifest_error(FORK_TOML.replace('version = "1.0"', 'version = "1.1"'), '`serde` differs')

    def test_renamed_dependency_must_point_at_the_fork(self):
        self.assert_manifest_error(FORK_TOML.replace('bashkit-monty-types', 'other'), 'must be package `bashkit-monty-types`')

    def test_changed_features_fail(self):
        self.assert_manifest_error(FORK_TOML.replace('hooks = ["monty-types/hooks"]', 'hooks = []'), '[features]')

    def test_target_dependencies_are_compared(self):
        (self.up / 'Cargo.toml').write_text(UPSTREAM_TOML + '\n[target.\'cfg(unix)\'.dependencies.libc]\nversion = "0.2"\n')
        self.assert_manifest_error(FORK_TOML, 'drops upstream `libc`')

    # A dependency the fork shares with bashkit's own crates has to be able to
    # move ahead of upstream: the two declarations are separate, so holding the
    # fork at upstream's version puts two majors of the same crate in the
    # graph. Marked divergence is how that is recorded, the same way a source
    # edit is.
    def test_marked_dependency_version_divergence_passes(self):
        fork = FORK_TOML.replace(
            'serde = { version = "1.0", features = ["derive"] }',
            '# bashkit fork: shared with bashkit, moved ahead of upstream\n'
            'serde = { version = "1.1", features = ["derive"] }',
        )
        (self.fork / 'Cargo.toml').write_text(fork)
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_marker_on_the_dependency_line_passes(self):
        fork = FORK_TOML.replace(
            'serde = { version = "1.0", features = ["derive"] }',
            'serde = { version = "1.1", features = ["derive"] }  '
            '# bashkit fork: moved ahead of upstream',
        )
        (self.fork / 'Cargo.toml').write_text(fork)
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_marker_does_not_excuse_adding_or_dropping_a_dependency(self):
        # Divergence in a *version* is reviewable; a dependency upstream does
        # not have is a different crate in the graph, and re-sync cannot
        # reconcile it. The marker must not reach those.
        added = FORK_TOML.replace(
            '[features]',
            '# bashkit fork: why\nevil = "1"\n\n[features]',
        )
        self.assert_manifest_error(added, 'adds `evil`')
        dropped = FORK_TOML.replace(
            'serde = { version = "1.0", features = ["derive"] }\n',
            '# bashkit fork: why\n',
        )
        self.assert_manifest_error(dropped, 'drops upstream `serde`')

    def test_marker_for_a_dependency_that_matches_upstream_is_stale(self):
        # After a re-sync the fork's version catches up and the note is dead
        # weight; left in place it would silently excuse the next bump.
        fork = FORK_TOML.replace(
            'serde = { version = "1.0", features = ["derive"] }',
            '# bashkit fork: stale note\nserde = { version = "1.0", features = ["derive"] }',
        )
        self.assert_manifest_error(fork, '`serde` matches upstream')

    def test_a_comment_block_marks_only_the_dependency_below_it(self):
        # One dep's note must not excuse the next dep's drift.
        fork = FORK_TOML.replace(
            'monty-types = { package = "bashkit-monty-types", path = "../t", version = "0.20.3" }\n'
            'serde = { version = "1.0", features = ["derive"] }',
            '# bashkit fork: note for monty-types\n'
            'monty-types = { package = "bashkit-monty-types", path = "../t", version = "0.20.3" }\n'
            'serde = { version = "1.1", features = ["derive"] }',
        )
        (self.fork / 'Cargo.toml').write_text(fork)
        errors = cmf.check_crate(self.fork, self.up)
        self.assertTrue(any('`serde` differs' in e for e in errors), errors)

    def test_marker_in_a_features_table_does_not_mark_a_dependency(self):
        # `[features]` entries look like `name = ...` too; only dependency
        # tables may carry a dependency marker.
        fork = FORK_TOML.replace(
            'serde = { version = "1.0", features = ["derive"] }',
            'serde = { version = "1.1", features = ["derive"] }',
        ).replace(
            'hooks = ["monty-types/hooks"]\n',
            '# bashkit fork: not a dependency marker\nserde = ["monty-types/hooks"]\n',
            1,
        )
        (self.fork / 'Cargo.toml').write_text(fork)
        errors = cmf.check_crate(self.fork, self.up)
        self.assertTrue(any('`serde` differs' in e for e in errors), errors)

    def test_a_marker_does_not_reach_the_same_crate_in_another_table(self):
        # A marker is scoped to the table it sits in. `[dependencies] serde`
        # and `[build-dependencies] serde` are different positions in the
        # build -- the second runs at compile time -- so a note on one must
        # not silence drift in the other.
        (self.up / 'Cargo.toml').write_text(
            UPSTREAM_TOML + '\n[build-dependencies.serde]\nversion = "1.0"\n'
        )
        (self.fork / 'Cargo.toml').write_text(
            FORK_TOML.replace(
                'serde = { version = "1.0", features = ["derive"] }',
                '# bashkit fork: runtime serde moved ahead of upstream\n'
                'serde = { version = "1.1", features = ["derive"] }',
            )
            + '\n[build-dependencies]\nserde = { version = "1.2" }\n'
        )
        errors = cmf.check_crate(self.fork, self.up)
        self.assertTrue(
            any('[build-dependencies]' in e and '`serde` differs' in e for e in errors),
            errors,
        )
        self.assertFalse(
            [e for e in errors if e.startswith('fork/Cargo.toml: [dependencies] `serde`')],
            errors,
        )

    # Where a dependency comes from is not a version, so a marker cannot excuse
    # it: re-sync cannot reconcile a different source, and an identical version
    # says nothing about what was actually built.
    def test_source_substitution_fails_even_with_identical_version(self):
        self.assert_manifest_error(
            FORK_TOML.replace(
                'serde = { version = "1.0", features = ["derive"] }',
                'serde = { version = "1.0", features = ["derive"], path = "../evil" }',
            ),
            '`serde` comes from',
        )

    def test_a_marker_does_not_excuse_a_source_substitution(self):
        self.assert_manifest_error(
            FORK_TOML.replace(
                'serde = { version = "1.0", features = ["derive"] }',
                '# bashkit fork: why\n'
                'serde = { version = "1.0", features = ["derive"], git = "https://e.example/s" }',
            ),
            '`serde` comes from',
        )

    def test_repackaging_a_dependency_fails(self):
        # `package = ...` swaps which crate is built while the version and
        # features still read as upstream's; only the declared renames may.
        self.assert_manifest_error(
            FORK_TOML.replace(
                'serde = { version = "1.0", features = ["derive"] }',
                'serde = { package = "evil-serde", version = "1.0", features = ["derive"] }',
            ),
            '`serde` comes from',
        )

    def test_a_target_table_marker_is_keyed_like_dep_tables(self):
        # `marked_deps` reads the raw header text while `manifest_errors` keys
        # tables from parsed TOML; a quoted cfg must normalize to the same key
        # in both, or a marker in a target table would never match.
        cfg = "cfg(all(target_arch = \"wasm32\", target_os = \"unknown\"))"
        (self.up / 'Cargo.toml').write_text(
            UPSTREAM_TOML + f"\n[target.'{cfg}'.dependencies.web-time]\nversion = \"1.0\"\n"
        )
        marked = FORK_TOML + (
            f"\n[target.'{cfg}'.dependencies]\n"
            '# bashkit fork: moved ahead of upstream\n'
            'web-time = { version = "1.1" }\n'
        )
        (self.fork / 'Cargo.toml').write_text(marked)
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

        # And without the marker the same drift is still reported.
        (self.fork / 'Cargo.toml').write_text(
            FORK_TOML
            + f"\n[target.'{cfg}'.dependencies]\nweb-time = {{ version = \"1.1\" }}\n"
        )
        errors = cmf.check_crate(self.fork, self.up)
        self.assertTrue(any('`web-time` differs' in e for e in errors), errors)

    def test_the_declared_rename_keeps_its_path_and_package(self):
        # monty-types legitimately points at the sibling fork by path, which
        # the source check must not flag.
        self.assertEqual(cmf.check_crate(self.fork, self.up), [])

    def test_pins_cover_both_forks(self):
        self.assertEqual(set(cmf.UPSTREAM), {'bashkit-monty', 'bashkit-monty-types'})
        for name, sha in cmf.UPSTREAM.values():
            self.assertRegex(sha, r'^[0-9a-f]{64}$', name)


if __name__ == '__main__':
    unittest.main()

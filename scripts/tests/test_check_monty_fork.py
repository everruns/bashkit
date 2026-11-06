"""Tests for scripts/check_monty_fork.py (offline, synthetic upstream trees)."""

from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_monty_fork as cmf  # noqa: E402

UPSTREAM_SRC = 'fn a() {}\nfn b() {\n    1\n}\nfn c() {}\n'


class CheckMontyForkTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.up = base / 'up'
        self.fork = base / 'fork'
        for d in (self.up, self.fork):
            (d / 'src').mkdir(parents=True)
            (d / 'src' / 'lib.rs').write_text(UPSTREAM_SRC)
        (self.fork / 'Cargo.toml').write_text('[package]\n')

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

    def test_pins_cover_both_forks(self):
        self.assertEqual(set(cmf.UPSTREAM), {'bashkit-monty', 'bashkit-monty-types'})
        for name, sha in cmf.UPSTREAM.values():
            self.assertRegex(sha, r'^[0-9a-f]{64}$', name)


if __name__ == '__main__':
    unittest.main()

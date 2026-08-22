"""Lock the release/CI parity checker to the drift that stalled v0.17.0's npm publish."""

from __future__ import annotations

import pathlib
import sys
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

import check_workflow_parity as parity  # noqa: E402

CI_WORKFLOW = """\
jobs:
  build-and-test:
    strategy:
      matrix:
        include:
          - runtime: node
            version: "20"
          - runtime: node
            version: "22"
          - runtime: bun
            version: "latest"
    steps:
      - name: Run ava tests (Node 22+ only)
        if: matrix.runtime == 'node' && matrix.version != '20'
        run: pnpm test
      - name: Run runtime-compat tests
        if: matrix.runtime == 'node'
        run: node --test __test__/runtime-compat/*.test.mjs
"""

RELEASE_GATED = """\
jobs:
  test-js:
    strategy:
      matrix:
        node: ["20", "22"]
    steps:
      - name: Test bindings (Node 22+ only)
        if: matrix.node != '20'
        run: pnpm test
      - name: Test bindings - runtime compat (Node 20)
        if: matrix.node == '20'
        run: node --test __test__/runtime-compat/*.test.mjs
"""

RELEASE_UNGATED = """\
jobs:
  test-js:
    strategy:
      matrix:
        node: ["20", "22"]
    steps:
      - name: Test bindings
        run: pnpm test
"""

RELEASE_EXTRA_VERSION = """\
jobs:
  test-js:
    strategy:
      matrix:
        node: ["22", "18"]
    steps:
      - name: Test bindings
        run: pnpm test
"""

LANE = {
    "runtime": "node",
    "ci": [("ci.yml", "build-and-test", "version", {"runtime": "node"})],
    "release": [("release.yml", "test-js", "node", {})],
}


class WorkflowParityTests(unittest.TestCase):
    def check(self, release_workflow: str) -> list[str]:
        with tempfile.TemporaryDirectory() as directory:
            workflows = pathlib.Path(directory)
            (workflows / "ci.yml").write_text(CI_WORKFLOW)
            (workflows / "release.yml").write_text(release_workflow)
            original = parity.WORKFLOWS
            parity.WORKFLOWS = workflows
            try:
                return parity.check_lane(LANE)
            finally:
                parity.WORKFLOWS = original

    def test_matching_gates_pass(self) -> None:
        self.assertEqual(self.check(RELEASE_GATED), [])

    def test_release_only_suite_on_a_version_is_reported(self) -> None:
        errors = self.check(RELEASE_UNGATED)
        self.assertEqual(len(errors), 1)
        self.assertIn("node 20", errors[0])
        self.assertIn("'ava'", errors[0])

    def test_release_only_version_is_reported(self) -> None:
        errors = self.check(RELEASE_EXTRA_VERSION)
        self.assertTrue(any("node 18" in error for error in errors))

    def test_toolchain_pins_must_agree_across_workflows(self) -> None:
        stable = "e081816240890017053eacbb1bdf337761dc5582"
        stale = "a" * 40
        with tempfile.TemporaryDirectory() as directory:
            workflows = pathlib.Path(directory)
            (workflows / "ci.yml").write_text(
                f"uses: dtolnay/rust-toolchain@{stable} # 1.95.0\n"
            )
            (workflows / "publish.yml").write_text(
                f"uses: dtolnay/rust-toolchain@{stale} # 1.95.0\n"
            )
            toolchain = workflows / "rust-toolchain.toml"
            toolchain.write_text(textwrap.dedent('[toolchain]\nchannel = "1.95.0"\n'))

            errors = parity.check_toolchain_pins(workflows, toolchain)

        self.assertEqual(len(errors), 1)
        self.assertIn("several SHAs", errors[0])

    def test_toolchain_channel_must_be_referenced_by_a_workflow(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workflows = pathlib.Path(directory)
            (workflows / "ci.yml").write_text(
                "uses: dtolnay/rust-toolchain@%s # 1.94.0\n" % ("b" * 40)
            )
            toolchain = workflows / "rust-toolchain.toml"
            toolchain.write_text('[toolchain]\nchannel = "1.95.0"\n')

            errors = parity.check_toolchain_pins(workflows, toolchain)

        self.assertEqual(len(errors), 1)
        self.assertIn("1.95.0", errors[0])

    def test_repository_workflows_are_in_parity(self) -> None:
        """The live check: this is what CI enforces on every push."""
        errors = []
        for lane in parity.LANES:
            errors += parity.check_lane(lane)
        errors += parity.check_toolchain_pins()
        self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()

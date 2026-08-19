"""Release orchestration must publish every public package."""

from __future__ import annotations

import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


def _ci_jobs() -> dict[str, str]:
    """Split .github/workflows/ci.yml into {job name: raw yaml body}.

    Hand-rolled rather than PyYAML: the Lint job runs these tests with the
    runner's system python and must not depend on a third-party package.
    """
    text = (ROOT / ".github/workflows/ci.yml").read_text()
    body = text.split("\njobs:\n", 1)[1]

    jobs: dict[str, str] = {}
    current = None
    for line in body.splitlines():
        header = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if header:
            current = header.group(1)
            jobs[current] = ""
        elif current is not None:
            jobs[current] += line + "\n"
    return jobs


class ReleaseWorkflowTests(unittest.TestCase):
    def test_release_dispatches_every_publish_workflow(self) -> None:
        workflow = (ROOT / ".github/workflows/release.yml").read_text()

        for publish_workflow in (
            "publish.yml",
            "publish-python.yml",
            "publish-js.yml",
            "publish-wasm.yml",
        ):
            with self.subTest(publish_workflow=publish_workflow):
                self.assertIn(
                    f'gh workflow run {publish_workflow} --ref "$TAG"', workflow
                )

    def test_release_dispatch_targets_exist(self) -> None:
        workflow = (ROOT / ".github/workflows/release.yml").read_text()

        for dispatch_target in re.findall(r"gh workflow run ([^ ]+)", workflow):
            with self.subTest(dispatch_target=dispatch_target):
                self.assertTrue(
                    (ROOT / ".github/workflows" / dispatch_target).is_file()
                )

    def test_public_package_versions_match_workspace(self) -> None:
        cargo_manifest = (ROOT / "Cargo.toml").read_text()
        match = re.search(r'^version = "([^"]+)"$', cargo_manifest, re.MULTILINE)
        self.assertIsNotNone(match)
        workspace_version = match.group(1)

        for package_manifest in (
            "crates/bashkit-js/package.json",
            "crates/bashkit-wasm/package.json",
        ):
            manifest = (ROOT / package_manifest).read_text()
            with self.subTest(package_manifest=package_manifest):
                self.assertIn(f'"version": "{workspace_version}"', manifest)

    def test_cli_publish_proxy_uses_the_published_core(self) -> None:
        justfile = (ROOT / "justfile").read_text()

        self.assertIn(r's/path = "\.\.\/bashkit", //', justfile)

    def test_web_ci_exercises_release_wasm_optimization(self) -> None:
        build_script = (ROOT / "crates/bashkit-wasm/scripts/build.sh").read_text()
        ci_workflow = (ROOT / ".github/workflows/ci.yml").read_text()

        for stable_feature in (
            "--enable-bulk-memory",
            "--enable-nontrapping-float-to-int",
            "--enable-sign-ext",
        ):
            self.assertIn(stable_feature, build_script)
        self.assertNotIn("--all-features", build_script)
        self.assertIn("scripts/install-binaryen-ci.sh", ci_workflow)


class CiHangGuardTest(unittest.TestCase):
    """CI must fail fast instead of hanging until GitHub's 6h job limit.

    Regression guard for the 2026-08-18 main-red run, where `apt-get update`
    stalled against an Ubuntu mirror and burned 6h before being cancelled.
    """

    def test_binaryen_install_does_not_depend_on_ubuntu_mirrors(self) -> None:
        for workflow in (
            ".github/workflows/ci.yml",
            ".github/workflows/publish-wasm.yml",
        ):
            text = (ROOT / workflow).read_text()
            with self.subTest(workflow=workflow):
                self.assertNotIn("apt-get install -y binaryen", text)
                self.assertIn("scripts/install-binaryen-ci.sh", text)

    def test_binaryen_installer_pins_archive_digests(self) -> None:
        script = (ROOT / "scripts/install-binaryen-ci.sh").read_text()

        self.assertIn("expected_sha256", script)
        self.assertIn("binaryen/releases/download", script)

    def test_every_ci_job_has_a_timeout(self) -> None:
        jobs = _ci_jobs()

        missing = sorted(
            name for name, body in jobs.items() if "timeout-minutes:" not in body
        )
        self.assertEqual(missing, [], f"CI jobs without timeout-minutes: {missing}")

    def test_check_gate_covers_every_other_ci_job(self) -> None:
        jobs = _ci_jobs()
        expected = sorted(name for name in jobs if name != "check")

        gate = jobs["check"]
        needs = re.search(r"needs: \[([^\]]*)\]", gate)
        assert needs is not None
        self.assertEqual(sorted(n.strip() for n in needs.group(1).split(",")), expected)

        for name in expected:
            with self.subTest(job=name):
                self.assertIn(f"needs.{name}.result", gate)


if __name__ == "__main__":
    unittest.main()

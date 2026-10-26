"""Release orchestration must publish every public package."""

from __future__ import annotations

import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]

# Single source of truth for the pinned wasm-opt install, shared by CI and the
# release workflow. See the script's header for why apt is not used.
INSTALLER = "scripts/install-binaryen.sh"

# registry.npmjs.org serves packuments with `cache-control: public,
# max-age=300`, so a CDN edge can answer with the pre-publish document for
# five minutes. Any post-publish verification has to outlast that.
NPM_PACKUMENT_MAX_AGE_SECONDS = 300


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

    def test_publish_js_skips_ava_on_node_20(self) -> None:
        """ava >= 8 needs Node 22.20+; publish-js must split like ci's js.yml.

        The release path is the only place this gap shows up, and it shows up
        after the tag exists (v0.17.0 shipped to crates.io and PyPI while npm
        stalled on it).
        """
        publish = (ROOT / ".github/workflows/publish-js.yml").read_text()

        active = "\n".join(
            line for line in publish.splitlines() if not line.lstrip().startswith("#")
        )
        ava_steps = re.findall(
            r"^      - name: Test bindings[^\n]*\n(?:(?:        .*)?\n)*",
            active,
            re.MULTILINE,
        )
        self.assertTrue(ava_steps)
        for step in ava_steps:
            with self.subTest(step=step.splitlines()[0]):
                if "ava" in step or "pnpm test" in step:
                    self.assertIn("matrix.node != '20'", step)

    def test_npm_publish_verification_outlasts_the_registry_cache(self) -> None:
        """v0.18.1 published to npm and the release still went red.

        `npm publish` succeeded at 00:45:29 (run 35291306179 logs
        `+ @everruns/bashkit@0.18.1` and a signed provenance statement), but
        the verification poll read `0.18.0` on all 24 attempts and failed at
        00:47:32. npm moved the `latest` tag at 00:48:10 -- 38 seconds after
        the poll gave up, and 161 seconds after the publish returned.

        The poll is not measuring the publish, it is racing a cache: npm's own
        publish output says the package "may take a few minutes to become
        available", and registry.npmjs.org serves packuments with
        `cache-control: public, max-age=300`, so a CDN edge can keep answering
        with the pre-publish document for five minutes. A window shorter than
        that reports a false failure on a release that actually shipped, which
        is worse than no check: it leaves a red release nobody can act on.

        (Retrying harder inside a short window does not help, and no npm flag
        avoids this -- `npm view` already forces a staleness check, it sets
        `preferOnline: true` itself. Only the window length fixes it.)
        """
        publish = (ROOT / ".github/workflows/publish-js.yml").read_text()
        step = re.search(
            r"^      - name: Verify npm publish\n(?:(?:        .*)?\n)*",
            publish,
            re.MULTILINE,
        )
        self.assertIsNotNone(step, "publish-js.yml lost its npm verification step")
        step = step.group(0)

        attempts = re.search(r"^\s*ATTEMPTS=(\d+)$", step, re.MULTILINE)
        sleep_seconds = re.search(r"^\s*SLEEP_SECONDS=(\d+)$", step, re.MULTILINE)
        self.assertIsNotNone(attempts, "verification lost its attempt count")
        self.assertIsNotNone(sleep_seconds, "verification lost its sleep interval")

        window = int(attempts.group(1)) * int(sleep_seconds.group(1))
        self.assertGreaterEqual(
            window,
            2 * NPM_PACKUMENT_MAX_AGE_SECONDS,
            f"npm publish verification polls for {window}s; the registry "
            f"caches packuments for {NPM_PACKUMENT_MAX_AGE_SECONDS}s, so the "
            f"window must clear that with margin or releases go red on a "
            f"publish that succeeded",
        )

        # The check still has to be a real one: poll the registry, and fail
        # the job if the version never lands.
        self.assertIn("npm view @everruns/bashkit", step)
        self.assertIn("exit 1", step)

    def test_release_check_verifies_the_exact_unpublished_package_graph(self) -> None:
        justfile = (ROOT / "justfile").read_text()
        recipe = justfile.split("\nrelease-check:\n", 1)[1].split("\n# Create and push", 1)[0]

        # New internal dependencies are absent from crates.io until publication.
        # Cargo stages them together only when every release package is selected.
        for package in ("bashkit-cpython-wasm", "bashkit", "bashkit-cli", "bashkit-scripted-tool"):
            self.assertRegex(recipe, rf"-p {package}(?:\s|$)")
        self.assertIn("--dry-run", recipe)
        self.assertNotIn("--no-verify", recipe)
        self.assertNotIn("LATEST_CORE", recipe)
        self.assertNotIn("cli_publish_proxy", recipe)

    def test_homebrew_release_metadata_matches_repository_policy(self) -> None:
        workflow = (ROOT / ".github/workflows/cli-binaries.yml").read_text()

        self.assertIn('git config user.name "Mykhailo Chalyi"', workflow)
        self.assertIn('git config user.email "mike@chaliy.name"', workflow)
        self.assertNotIn("github-actions[bot]", workflow)
        license_match = re.search(r'^license = "([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.MULTILINE)
        self.assertIsNotNone(license_match)
        self.assertIn(f'license "{license_match.group(1)}"', workflow)

    def test_npm_publish_lanes_share_exact_archive_verification(self) -> None:
        for name in ("publish-js.yml", "publish-wasm.yml"):
            workflow = (ROOT / ".github/workflows" / name).read_text()
            with self.subTest(workflow=name):
                self.assertIn('python3 "$GITHUB_WORKSPACE/scripts/publish_npm.py" --tag "$TAG"', workflow)

    def test_npm_publish_lanes_use_trusted_publishing(self) -> None:
        """npm auth is GitHub OIDC trusted publishing, never a long-lived token.

        A rejected NPM_TOKEN made npm answer the 0.20.1 wasm publish with a
        misleading 404. setup-node's registry-url writes an .npmrc carrying a
        placeholder NODE_AUTH_TOKEN, so it stays off too.
        """
        for name, job in (("publish-js.yml", "release-js"), ("publish-wasm.yml", "publish-wasm")):
            workflow = (ROOT / ".github/workflows" / name).read_text()
            with self.subTest(workflow=name):
                self.assertNotIn("secrets.NPM_TOKEN", workflow)
                self.assertNotIn("NODE_AUTH_TOKEN", workflow)
                self.assertNotIn("registry-url:", workflow)
                body = workflow.split(f"\n  {job}:\n", 1)[1]
                self.assertIn("id-token: write", body.split("\n    steps:\n", 1)[0])
                self.assertIn("npm install -g npm@11.6.2", body)

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

        # CI must install wasm-opt the same way the release does, so the -Oz
        # pass that ships to npm is the one CI exercises. Both go through
        # scripts/install-binaryen.sh rather than each carrying its own copy
        # of an install command that can drift apart.
        publish_workflow = (ROOT / ".github/workflows/publish-wasm.yml").read_text()
        self.assertIn(INSTALLER, ci_workflow)
        self.assertIn(INSTALLER, publish_workflow)

    def test_binaryen_install_is_pinned_and_bounded(self) -> None:
        """`apt-get install binaryen` stalled indefinitely on two separate main
        runs (32170059933, 32218864077). With no step timeout each burned the
        6-hour job ceiling and took the whole run down as `cancelled`, so main
        went red twice for a mirror problem unrelated to any diff. Guard the
        three properties that fix has to keep."""
        installer = (ROOT / INSTALLER).read_text()

        # No apt: that is the mechanism that hung. The step may still be
        # *named* "Install binaryen"; what must not come back is installing it
        # through a package manager.
        for workflow in ("ci.yml", "publish-wasm.yml"):
            contents = (ROOT / ".github/workflows" / workflow).read_text()
            offenders = [
                line
                for line in contents.splitlines()
                if "binaryen" in line and ("apt-get" in line or "apt " in line)
            ]
            with self.subTest(workflow=workflow):
                self.assertEqual(offenders, [], f"{workflow} installs binaryen via apt")

        # Bounded: a stalled download must fail in minutes, not hours.
        self.assertIn("--max-time", installer)
        self.assertIn("--connect-timeout", installer)

        # Pinned version + checksum. The version keeps `-Oz` output
        # reproducible instead of tracking the runner image's Ubuntu; the
        # checksum is the only integrity check available, since binaryen
        # publishes no signature or checksum file for its release assets.
        self.assertRegex(installer, r'BINARYEN_VERSION:?="?\$\{BINARYEN_VERSION:-\d+\}')
        self.assertRegex(installer, r"BINARYEN_SHA256.*[0-9a-f]{64}")
        self.assertIn("sha256sum -c", installer)

    def test_wasm_build_jobs_cannot_hang_past_the_job_ceiling(self) -> None:
        """A job with no `timeout-minutes` inherits GitHub's 6-hour ceiling, so
        a stuck step wastes a runner for six hours and reports `cancelled`
        rather than a re-runnable failure. Both jobs that build the wasm bundle
        must bound themselves."""
        for workflow, job in (
            (".github/workflows/ci.yml", "wasm-web"),
            (".github/workflows/publish-wasm.yml", "build-wasm"),
        ):
            contents = (ROOT / workflow).read_text()
            with self.subTest(workflow=workflow, job=job):
                block = contents.split(f"\n  {job}:\n", 1)
                self.assertEqual(len(block), 2, f"job {job} not found in {workflow}")
                # Only the job's own header, up to its `steps:` key.
                header = block[1].split("\n    steps:", 1)[0]
                self.assertRegex(header, r"timeout-minutes: \d+")


if __name__ == "__main__":
    unittest.main()

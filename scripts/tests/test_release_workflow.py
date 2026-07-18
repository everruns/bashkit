"""Release orchestration must publish every public package."""

from __future__ import annotations

import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


class ReleaseWorkflowTests(unittest.TestCase):
    def test_release_dispatches_every_publish_workflow(self) -> None:
        workflow = (ROOT / ".github/workflows/release.yml").read_text()

        for publish_workflow in (
            "publish.yml",
            "publish-python.yml",
            "publish-js.yml",
            "publish-web.yml",
        ):
            with self.subTest(publish_workflow=publish_workflow):
                self.assertIn(
                    f'gh workflow run {publish_workflow} --ref "$TAG"', workflow
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


if __name__ == "__main__":
    unittest.main()

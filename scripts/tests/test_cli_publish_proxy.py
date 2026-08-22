"""Lock the release-check CLI packaging proxy to the published core's features."""

from __future__ import annotations

import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

from cli_publish_proxy import ManifestError, rewrite  # noqa: E402

MANIFEST = """\
[package]
name = "bashkit-cli"

[features]
default = ["http_client", "python", "tzdata"]
python = ["bashkit/python"]
tzdata = ["bashkit/tzdata"]

[dependencies]
bashkit = { path = "../bashkit", version = "0.17.0", default-features = false, features = ["http_client", "ring", "git", "python", "tzdata"] }
"""


class CliPublishProxyTests(unittest.TestCase):
    def test_drops_features_the_published_core_lacks(self) -> None:
        rewritten, kept, dropped = rewrite(MANIFEST, {"http_client", "git"})
        self.assertEqual(kept, ["http_client", "git"])
        self.assertEqual(dropped, ["ring", "python", "tzdata"])
        self.assertIn('features = ["http_client", "git"]', rewritten)
        self.assertNotIn('path = "../bashkit"', rewritten)
        self.assertNotIn('python = ["bashkit/python"]', rewritten)
        self.assertNotIn('tzdata = ["bashkit/tzdata"]', rewritten)
        self.assertIn('default = ["http_client"]', rewritten)

    def test_keeps_everything_when_core_has_all_features(self) -> None:
        available = {"http_client", "ring", "git", "python", "tzdata"}
        rewritten, kept, dropped = rewrite(MANIFEST, available)
        self.assertEqual(dropped, [])
        self.assertIn(
            'features = ["http_client", "ring", "git", "python", "tzdata"]', rewritten
        )
        self.assertIn('python = ["bashkit/python"]', rewritten)

    def test_rejects_a_manifest_without_the_dependency(self) -> None:
        with self.assertRaises(ManifestError):
            rewrite("[package]\nname = \"bashkit-cli\"\n", {"git"})

    def test_rejects_a_dependency_without_features(self) -> None:
        manifest = '[dependencies]\nbashkit = { path = "../bashkit", version = "0.17.0" }\n'
        with self.assertRaises(ManifestError):
            rewrite(manifest, {"git"})


if __name__ == "__main__":
    unittest.main()

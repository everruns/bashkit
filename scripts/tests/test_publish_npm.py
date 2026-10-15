"""A staged conflict succeeds only after the exact archive becomes public."""

import base64
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from publish_npm import publish


class PublishNpmTests(unittest.TestCase):
    def invoke(self, *, error="", remote=None, public_after=0, visible_tag="latest"):
        data = b"retained release archive"
        integrity = "sha512-" + base64.b64encode(hashlib.sha512(data).digest()).decode()
        calls = []
        views = 0

        def run(args):
            nonlocal views
            calls.append(args)
            if args[1] == "pack":
                self.assertIn("--ignore-scripts", args)
                return subprocess.CompletedProcess(
                    args,
                    0,
                    json.dumps([{"filename": "release.tgz", "integrity": integrity}]),
                    "",
                )
            if args[1] == "publish":
                self.assertEqual(args[2], "release.tgz")
                self.assertIn("--ignore-scripts", args)
                self.assertIn("--provenance", args)
                return subprocess.CompletedProcess(args, 1 if error else 0, "", error)
            self.assertEqual(args[1], "view")
            views += 1
            if views <= public_after:
                return subprocess.CompletedProcess(args, 1, "", "not public")
            value = (
                (remote or integrity)
                if args[3] == "dist.integrity"
                else ("1.2.3" if args[3] == f"dist-tags.{visible_tag}" else "old")
            )
            return subprocess.CompletedProcess(args, 0, json.dumps(value), "")

        with tempfile.TemporaryDirectory() as directory:
            previous = Path.cwd()
            try:
                os.chdir(directory)
                Path("package.json").write_text(
                    json.dumps({"name": "@example/release", "version": "1.2.3"})
                )
                Path("release.tgz").write_bytes(data)
                publish(
                    "latest",
                    run=run,
                    sleep=lambda _: None,
                    now=lambda: 0,
                    attempts=2,
                    delay=1,
                )
            finally:
                os.chdir(previous)
        return calls

    def test_staged_conflict_waits_for_exact_public_archive(self):
        calls = self.invoke(
            error='npm error E409 Cannot publish over previously staged version "1.2.3".',
            public_after=1,
        )
        self.assertEqual(sum(call[1] == "publish" for call in calls), 1)
        self.assertTrue(any(call[1] == "view" for call in calls))

    def test_successful_upload_still_verifies_public_identity(self):
        calls = self.invoke()
        self.assertTrue(any(call[1] == "view" for call in calls))

    def test_wrong_public_archive_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "integrity"):
            self.invoke(remote="sha512-wrong")

    def test_permission_failure_is_not_recovered(self):
        with self.assertRaisesRegex(RuntimeError, "publish failed"):
            self.invoke(error="npm error E403 authentication required")

    def test_missing_public_version_stops_at_bound(self):
        with self.assertRaisesRegex(RuntimeError, "not verified"):
            self.invoke(
                error="npm error E409 Cannot publish over previously staged version",
                public_after=100,
            )

    def test_wrong_dist_tag_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "not verified"):
            self.invoke(visible_tag="next")


if __name__ == "__main__":
    unittest.main()

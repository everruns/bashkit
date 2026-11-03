"""npm publish source resolution: default ref vs. re-publishing a release tag."""

from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "resolve_publish_source.sh"


def git(cwd, *args):
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout.strip()


class ResolvePublishSourceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.tmp.name)
        origin = root / "origin"
        origin.mkdir()
        git(origin, "init", "-q", "-b", "main")
        git(origin, "config", "user.email", "t@example.com")
        git(origin, "config", "user.name", "t")
        git(origin, "commit", "-q", "--allow-empty", "-m", "release")
        git(origin, "tag", "v1.2.3")
        self.tag_sha = git(origin, "rev-parse", "HEAD")
        git(origin, "commit", "-q", "--allow-empty", "-m", "after")
        self.main_sha = git(origin, "rev-parse", "HEAD")
        git(origin, "checkout", "-q", "-b", "side", self.tag_sha)
        git(origin, "commit", "-q", "--allow-empty", "-m", "off main")
        self.side_sha = git(origin, "rev-parse", "HEAD")
        git(origin, "tag", "v9.9.9")
        git(origin, "checkout", "-q", "main")
        self.work = root / "work"
        git(root, "clone", "-q", str(origin), str(self.work))
        self.output = root / "out"

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def resolve(self, *, tag="", sha, ref):
        self.output.write_text("")
        env = {
            **os.environ,
            "INPUT_TAG": tag,
            "GITHUB_SHA": sha,
            "GITHUB_REF": ref,
            "GITHUB_OUTPUT": str(self.output),
        }
        result = subprocess.run(
            ["bash", str(SCRIPT)], cwd=self.work, env=env, capture_output=True, text=True
        )
        outputs = dict(
            line.split("=", 1) for line in self.output.read_text().splitlines()
        )
        return result.returncode, outputs, result.stderr

    def test_release_tag_ref_publishes_latest(self) -> None:
        code, out, _ = self.resolve(sha=self.tag_sha, ref="refs/tags/v1.2.3")
        self.assertEqual(code, 0)
        self.assertEqual(out, {"sha": self.tag_sha, "dist_tag": "latest", "version": "1.2.3"})

    def test_branch_ref_publishes_next(self) -> None:
        code, out, _ = self.resolve(sha=self.main_sha, ref="refs/heads/main")
        self.assertEqual(code, 0)
        self.assertEqual(out, {"sha": self.main_sha, "dist_tag": "next", "version": ""})

    def test_branch_named_like_a_tag_is_not_latest(self) -> None:
        _, out, _ = self.resolve(sha=self.main_sha, ref="refs/heads/v1.2.3")
        self.assertEqual(out["dist_tag"], "next")

    def test_tag_input_from_main_builds_the_tag_commit(self) -> None:
        code, out, _ = self.resolve(tag="v1.2.3", sha=self.main_sha, ref="refs/heads/main")
        self.assertEqual(code, 0)
        self.assertEqual(out, {"sha": self.tag_sha, "dist_tag": "latest", "version": "1.2.3"})

    def test_tag_input_rejects_other_workflow_refs(self) -> None:
        code, out, err = self.resolve(tag="v1.2.3", sha=self.side_sha, ref="refs/heads/side")
        self.assertEqual(code, 1)
        self.assertEqual(out, {})
        self.assertIn("must be v1.2.3 or current main", err)

    def test_tag_input_must_look_like_a_release(self) -> None:
        code, out, err = self.resolve(tag="main", sha=self.main_sha, ref="refs/heads/main")
        self.assertEqual(code, 1)
        self.assertEqual(out, {})
        self.assertIn("vMAJOR.MINOR.PATCH", err)

    def test_source_off_main_is_rejected(self) -> None:
        code, out, err = self.resolve(tag="v9.9.9", sha=self.side_sha, ref="refs/tags/v9.9.9")
        self.assertEqual(code, 1)
        self.assertEqual(out, {})
        self.assertIn("not reachable from origin/main", err)


if __name__ == "__main__":
    unittest.main()

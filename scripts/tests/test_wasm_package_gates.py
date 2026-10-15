"""Browser package CI and publication must gate the same artifact contract."""
from pathlib import Path
import tomllib
import json
import os
import shutil
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]


class WasmPackageGatesTests(unittest.TestCase):
    def test_size_profile_retains_release_safety_policy(self):
        manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
        profile = manifest["profile"]["wasm-release"]
        self.assertEqual(profile["inherits"], "release")
        self.assertEqual(profile["opt-level"], "s")
        self.assertEqual(profile["lto"], "fat")
        self.assertNotIn("panic", profile)
        self.assertNotIn("overflow-checks", profile)
        self.assertEqual(manifest["profile"]["release"]["panic"], "unwind")

    def test_source_archive_cannot_claim_enclosing_repository_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            root = parent / "archive"
            script = root / "crates/bashkit-wasm/scripts/build-info.mjs"
            script.parent.mkdir(parents=True)
            shutil.copy(ROOT / "crates/bashkit-wasm/scripts/build-info.mjs", script)
            pkg = root / "pkg"
            pkg.mkdir()
            (pkg / "package.json").write_text('{"version":"1.2.3"}')
            (pkg / "bashkit_wasm_bg.wasm").write_bytes(b"wasm-fixture")
            tools = parent / "bin"
            tools.mkdir()
            (tools / "git").write_text(
                '#!/bin/sh\ncase "$*" in\n'
                '"rev-parse --show-toplevel") echo "$GIT_ROOT" ;;\n'
                '"rev-parse HEAD") printf "%040d\\n" 1 ;;\n'
                '"status --porcelain") exit 0 ;;\nesac\n')
            for name in ("rustc", "wasm-bindgen"):
                (tools / name).write_text(f'#!/bin/sh\necho "{name} fixture"\n')
            for tool in tools.iterdir():
                tool.chmod(0o755)
            env = dict(os.environ, PATH=f'{tools}:{os.environ["PATH"]}')
            for git_root, expected in ((parent, None), (root, "0" * 39 + "1")):
                env["GIT_ROOT"] = str(git_root)
                subprocess.run(["node", str(script), str(pkg), "wasm-release",
                                "terminal", "wasm-opt fixture"],
                               cwd=parent, env=env, check=True, capture_output=True)
                info = json.loads((pkg / "build-info.json").read_text())
                self.assertEqual(info["sourceRevision"], expected)
                self.assertEqual(info["sourceDirty"], None if expected is None else False)
                self.assertEqual(info["features"], ["terminal"])

    def test_ci_and_publish_gate_compatibility_and_transfer_bytes(self):
        gates = []
        for workflow, job in (("ci.yml", "wasm-web"),
                              ("publish-wasm.yml", "build-wasm")):
            doc = yaml.safe_load((ROOT / ".github/workflows" / workflow).read_text())
            steps = doc["jobs"][job]["steps"]
            gate = next(s["run"] for s in steps
                        if s.get("name") == "Browser compatibility and transfer-size gates")
            self.assertIn("bench/compat.mjs", gate)
            self.assertIn("bench/artifact.mjs crates/bashkit-wasm/pkg --check", gate)
            gates.append(gate)
        self.assertEqual(*gates)


if __name__ == "__main__":
    unittest.main()

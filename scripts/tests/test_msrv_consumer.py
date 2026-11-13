"""Guard the independent consumer/MSRV test, not just the workspace build."""
import importlib.util
from pathlib import Path
import tomllib
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("msrv", ROOT / "scripts/check-msrv-consumer.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class MsrvConsumerTest(unittest.TestCase):
    def test_feature_profiles(self):
        for profile in ["lean", "default", "all", "wasm", "wasi", "browser", "component"]:
            with self.subTest(profile=profile):
                doc = tomllib.loads(MODULE.manifest(ROOT, profile))
                self.assertEqual(doc["package"]["rust-version"], "1.96")
                dep = doc["dependencies"]["bashkit"]
                self.assertEqual(dep["default-features"], profile not in ("lean", "browser", "component"))
                self.assertEqual(Path(dep["path"]), ROOT / "crates/bashkit")
                self.assertEqual("wasm_js" in dep["features"], profile in ("all", "wasm", "browser"))
        features = tomllib.loads(MODULE.manifest(ROOT, "all"))["dependencies"]["bashkit"]["features"]
        actual = tomllib.loads((ROOT / "crates/bashkit/Cargo.toml").read_text())["features"]
        self.assertEqual(set(features), set(actual))

    def test_profile_commands_and_randomness_backends(self):
        for profile in ["lean", "default", "all", "wasm", "wasi", "browser", "component"]:
            with self.subTest(profile=profile):
                calls = []

                def run(command, *, cwd, env, check):
                    calls.append((command, env.copy()))

                with patch("sys.argv", ["check-msrv-consumer.py", "--profile", profile]), patch.object(MODULE.subprocess, "run", side_effect=run):
                    MODULE.main()
                command, env = calls[-1]
                self.assertEqual(command[-1], "--locked")
                if profile in ("lean", "default"):
                    self.assertIn("run", command)
                else:
                    self.assertIn("check", command)
                if profile in ("wasm", "browser", "component"):
                    self.assertIn("wasm32-unknown-unknown", command)
                elif profile == "wasi":
                    self.assertIn("wasm32-wasip2", command)
                else:
                    self.assertNotIn("--target", command)
                if profile in ("wasm", "browser"):
                    self.assertIn('getrandom_backend="wasm_js"', env["RUSTFLAGS"])
                elif profile == "component":
                    self.assertIn('getrandom_backend="custom"', env["RUSTFLAGS"])

    def test_resolution_is_outside_workspace_without_repository_lockfile(self):
        calls = []

        def run(command, *, cwd, env, check):
            calls.append(command)
            self.assertNotIn(ROOT, cwd.parents)
            self.assertTrue(check)
            self.assertTrue((cwd / "src/main.rs").is_file())
            if command[-1] == "generate-lockfile":
                self.assertFalse((cwd / "Cargo.lock").exists())
                (cwd / "Cargo.lock").write_text("# independently generated")
            else:
                self.assertEqual(command[-1], "--locked")
                self.assertTrue((cwd / "Cargo.lock").is_file())

        with patch("sys.argv", ["check-msrv-consumer.py"]), patch.object(MODULE.subprocess, "run", side_effect=run):
            MODULE.main()
        self.assertEqual(calls[0], ["cargo", "+1.96.0", "generate-lockfile"])
        self.assertEqual(calls[1], ["cargo", "+1.96.0", "run", "--locked"])


if __name__ == "__main__":
    unittest.main()

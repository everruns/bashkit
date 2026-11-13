#!/usr/bin/env python3
"""Compile an independent Bashkit consumer without the repository lockfile.

The development toolchain must not silently become the consumer minimum.
Resolve afresh, then use that consumer's lockfile for compilation/execution.
This is not a supply-chain audit: run without project secrets or write tokens.
Fresh registry dependencies may execute build scripts outside the workspace vet set.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib


def manifest(repo, profile):
    features = []
    defaults = profile not in ("lean", "browser", "component")
    if profile == "all":
        features = sorted(tomllib.loads((repo / "crates/bashkit/Cargo.toml").read_text())["features"])
    elif profile == "wasm":
        features = ["jq", "terminal", "wasm_js"]
    elif profile == "browser":
        features = ["jq", "terminal", "wasm_js", "python", "typescript"]
    elif profile == "component":
        features = ["jq"]
    elif profile == "wasi":
        features = ["jq", "terminal"]
    extra = 'getrandom03 = { package = "getrandom", version = "0.3", features = ["wasm_js"] }\n' if profile == "browser" else ""
    return f'''[package]
name = "bashkit-msrv-consumer"
version = "0.0.0"
edition = "2024"
rust-version = "1.96"
[dependencies]
bashkit = {{ path = {json.dumps(str(repo / 'crates/bashkit'))}, default-features = {str(defaults).lower()}, features = {json.dumps(features)} }}
tokio = {{ version = "1", features = ["rt", "macros"] }}
{extra}'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--toolchain", default="1.96.0")
    parser.add_argument("--profile", choices=["lean", "default", "all", "wasm", "wasi", "browser", "component"], default="default")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="bashkit-consumer-") as tmp:
        root = Path(tmp)
        (root / "src").mkdir()
        (root / "Cargo.toml").write_text(manifest(repo, args.profile))
        (root / "src/main.rs").write_text('''#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut bash = bashkit::Bash::new();
    let result = bash.exec("printf '%s\\\\n' compatible").await.unwrap();
    assert_eq!(result.stdout, "compatible\\n");
    assert_eq!(result.exit_code, 0);
}
''')
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = env.get("BASHKIT_CONSUMER_TARGET_DIR", str(repo / "target" / f"consumer-{args.toolchain}"))
        cargo = ["cargo", f"+{args.toolchain}"]
        subprocess.run(cargo + ["generate-lockfile"], cwd=root, env=env, check=True)
        if args.profile in ("wasm", "wasi", "browser", "component"):
            target = "wasm32-unknown-unknown" if args.profile != "wasi" else "wasm32-wasip2"
            if args.profile in ("wasm", "browser"):
                env["RUSTFLAGS"] = env.get("RUSTFLAGS", "") + ' --cfg getrandom_backend="wasm_js"'
            elif args.profile == "component":
                env["RUSTFLAGS"] = env.get("RUSTFLAGS", "") + ' --cfg getrandom_backend="custom"'
            command = ["check", "--target", target]
        elif args.profile == "all":
            # Check optional runtime combinations without linking every interpreter.
            command = ["check"]
        else:
            command = ["run"]
        subprocess.run(cargo + command + ["--locked"], cwd=root, env=env, check=True)


if __name__ == "__main__":
    main()

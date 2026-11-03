#!/usr/bin/env python3
"""Keep the browser VM copy byte-close to the recorded ZapCode 1.5.1 source."""
import hashlib
import json
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parents[1] / "crates/bashkit/src/zapcode_browser"
native_versions = {package["version"] for package in tomllib.loads((root.parents[3] / "Cargo.lock").read_text())["package"] if package["name"] == "zapcode-core"}
if native_versions != {"1.5.1"}:
    raise SystemExit("update the browser source copy when updating native ZapCode")
expected = json.loads((root / "upstream-sha256.json").read_text())
actual_paths = {str(p.relative_to(root)) for p in root.rglob("*.rs")}
if actual_paths != {"mod.rs" if p == "lib.rs" else p for p in expected}:
    raise SystemExit("vendored file inventory changed")
for path, digest in expected.items():
    text = (root / ("mod.rs" if path == "lib.rs" else path)).read_text()
    text = text.replace("crate::zapcode_browser::", "crate::")
    if path in {"sandbox.rs", "trace.rs"}:
        text = text.replace("crate::time::", "std::time::")
    if hashlib.sha256(text.encode()).hexdigest() != digest:
        raise SystemExit(f"unrecorded ZapCode change: {path}")
print("browser ZapCode matches 1.5.1 plus namespace and platform-clock changes")

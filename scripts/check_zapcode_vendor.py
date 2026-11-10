#!/usr/bin/env python3
"""Keep the vendored ZapCode copy byte-close to the recorded 1.5.1 source.

Allowed differences, each reversed before hashing:
- namespace: `crate::` -> `crate::zapcode_vendor::`, lib.rs -> mod.rs
- platform clock: sandbox.rs/trace.rs read `crate::time::` (Bashkit's shim)
- `bashkit fork:` edits listed in FORK_EDITS (resource-limit fixes upstream
  has not shipped). Every marker in the tree must be listed here.
"""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / "crates/bashkit/src/zapcode_vendor"

PAD_CAP = (
    "                // bashkit fork: cap like `repeat`; unbounded take() aborted the host.\n"
    "                if target_len > 10_000_000 { return Err(ZapcodeError::AllocationLimitExceeded); }\n"
)
# (file, inserted text, number of occurrences). TM-TS-025: padStart/padEnd.
FORK_EDITS = [("vm/builtins.rs", PAD_CAP, 2)]

expected = json.loads((root / "upstream-sha256.json").read_text())
actual_paths = {str(p.relative_to(root)) for p in root.rglob("*.rs")}
if actual_paths != {"mod.rs" if p == "lib.rs" else p for p in expected}:
    raise SystemExit("vendored file inventory changed")

listed_markers = {}
for path, text, count in FORK_EDITS:
    listed_markers[path] = listed_markers.get(path, 0) + text.count("bashkit fork:") * count

for path, digest in expected.items():
    text = (root / ("mod.rs" if path == "lib.rs" else path)).read_text()
    if text.count("bashkit fork:") != listed_markers.get(path, 0):
        raise SystemExit(f"unlisted `bashkit fork:` edit in {path}")
    for edit_path, inserted, count in FORK_EDITS:
        if edit_path == path:
            if text.count(inserted) != count:
                raise SystemExit(f"fork edit drifted in {path}")
            text = text.replace(inserted, "")
    text = text.replace("crate::zapcode_vendor::", "crate::")
    if path in {"sandbox.rs", "trace.rs"}:
        text = text.replace("crate::time::", "std::time::")
    if hashlib.sha256(text.encode()).hexdigest() != digest:
        raise SystemExit(f"unrecorded ZapCode change: {path}")
print("vendored ZapCode matches 1.5.1 plus namespace, platform-clock and listed fork edits")

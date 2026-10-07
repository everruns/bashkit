"""Every tsc output the npm package lists must reach the publish job.

Regression: publish-js.yml carried only wrapper.js/.d.ts in its `js-stubs`
artifact, so `@everruns/bashkit@0.18.2` shipped without langchain.js, ai.js,
anthropic.js and openai.js even though package.json exports them. The publish
job never runs tsc itself; whatever the artifact omits is missing on npm.
"""

from __future__ import annotations

import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
JS = ROOT / "crates/bashkit-js"


def stub_paths() -> set[str]:
    workflow = (ROOT / ".github/workflows/publish-js.yml").read_text()
    block = re.search(r"name: js-stubs\n\s+path: \|\n((?:\s+crates/bashkit-js/\S+\n)+)", workflow)
    assert block, "js-stubs upload block not found"
    return {line.strip().removeprefix("crates/bashkit-js/") for line in block.group(1).splitlines()}


class JsPublishFilesTests(unittest.TestCase):
    def test_every_ts_entry_output_is_uploaded(self) -> None:
        tsconfig = json.loads((JS / "tsconfig.json").read_text())
        uploaded = stub_paths()
        for source in tsconfig["include"]:
            stem = source.removesuffix(".ts")
            for out in (f"{stem}.js", f"{stem}.d.ts"):
                self.assertIn(out, uploaded, f"{out} is built by tsc but not uploaded to publish")

    def test_every_listed_tsc_file_is_uploaded(self) -> None:
        package = json.loads((JS / "package.json").read_text())
        tsc_outputs = {
            f"{source.removesuffix('.ts')}{ext}"
            for source in json.loads((JS / "tsconfig.json").read_text())["include"]
            for ext in (".js", ".d.ts")
        }
        uploaded = stub_paths()
        for entry in package["files"]:
            if entry in tsc_outputs:
                self.assertIn(entry, uploaded, f"package.json lists {entry} but publish never receives it")

    def test_every_export_target_is_packaged(self) -> None:
        package = json.loads((JS / "package.json").read_text())
        files = set(package["files"])
        for subpath, targets in package["exports"].items():
            for target in targets.values():
                self.assertIn(target.removeprefix("./"), files, f"export {subpath} -> {target} not in files")


if __name__ == "__main__":
    unittest.main()

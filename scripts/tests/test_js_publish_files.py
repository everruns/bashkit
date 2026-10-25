"""Every tsc output the npm package lists must reach the publish job.

Regression: publish-js.yml carried only wrapper.js/.d.ts in its `js-stubs`
artifact, so `@everruns/bashkit@0.18.2` shipped without langchain.js, ai.js,
anthropic.js and openai.js even though package.json exports them. The publish
job never runs tsc itself; whatever the artifact omits is missing on npm.

Second regression, v0.20.0: `runtime-functions.ts` arrived in #2740 without
being added to tsconfig's `include`. tsc still emitted it, because `wrapper.ts`
imports it and TypeScript compiles a reachable import either way, so every
local build and `js.yml` stayed green. But the omission also made the file
invisible to the two checks above, which read their notion of "tsc output"
straight out of `include` -- so the guard written for the 0.18.2 failure could
not see the next instance of it. All fifteen `Publish JS` test jobs then died
on `ERR_MODULE_NOT_FOUND` for runtime-functions.js and npm stalled at 0.19.0.

`include` is therefore checked against the sources on disk, so no entry point
can be absent from it and silently narrow the two checks that depend on it.
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
    def test_tsconfig_includes_every_ts_source(self) -> None:
        """`include` is the root both checks below trust; it must be complete.

        A source missing here is not a build failure -- tsc follows imports --
        so nothing else reveals it. It just quietly shrinks `tsc_outputs`.
        `tsconfig.check.json` overrides `include` rather than extending it, so
        it needs the same entries to type-check each source in its own right.
        """
        sources = {p.name for p in JS.glob("*.ts") if not p.name.endswith(".d.ts")}
        for config in ("tsconfig.json", "tsconfig.check.json"):
            included = set(json.loads((JS / config).read_text())["include"])
            for source in sorted(sources):
                self.assertIn(
                    source,
                    included,
                    f"{source} is a package source but is not in {config} include",
                )

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

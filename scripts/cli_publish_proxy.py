#!/usr/bin/env python3
"""Rewrite a disposable bashkit-cli manifest so it packages against the
published core crate.

`cargo publish --dry-run -p bashkit-cli` cannot resolve the core version being
released, because it is not on crates.io yet. `just release-check` therefore
packages a throwaway copy of the workspace against the latest *published* core
as a structural proxy. That copy may only request features the published core
already exposes, so this script drops the local path dependency and any feature
the published core lacks (features added in the current cycle, plus `python`,
which predates Monty's crates.io publication). The exact new core package is
validated by its own dry-run, so nothing here weakens the release gate.
"""

from __future__ import annotations

import json
import re
import sys
import urllib.request

API = "https://crates.io/api/v1/crates/bashkit/{version}"
UA = "bashkit-release-check (https://github.com/everruns/bashkit)"
DEPENDENCY = re.compile(r"^bashkit = \{[^}\n]*\}$", re.MULTILINE)
FEATURES = re.compile(r"features = \[([^\]]*)\]")


class ManifestError(Exception):
    """The copied manifest does not have the shape the proxy can rewrite."""


def published_features(version: str) -> set[str]:
    request = urllib.request.Request(API.format(version=version), headers={"User-Agent": UA})
    with urllib.request.urlopen(request, timeout=30) as response:
        payload = json.load(response)
    return set(payload["version"]["features"])


def rewrite(manifest: str, available: set[str]) -> tuple[str, list[str], list[str]]:
    """Return the rewritten manifest plus the kept and dropped feature names."""
    dependency = DEPENDENCY.search(manifest)
    if dependency is None:
        raise ManifestError("bashkit dependency line not found")
    line = dependency.group(0)
    requested = FEATURES.search(line)
    if requested is None:
        raise ManifestError("bashkit dependency has no feature list")

    names = [name.strip().strip('"') for name in requested.group(1).split(",") if name.strip()]
    kept = [name for name in names if name in available]
    dropped = [name for name in names if name not in available]

    line = (
        line[: requested.start()]
        + "features = [%s]" % ", ".join('"%s"' % name for name in kept)
        + line[requested.end() :]
    )
    line = line.replace('path = "../bashkit", ', "")
    manifest = manifest[: dependency.start()] + line + manifest[dependency.end() :]

    # Drop CLI features that forward to a core feature the published core lacks.
    for name in dropped:
        manifest = re.sub(
            r'^%s = \["bashkit/%s"\]\n' % (re.escape(name), re.escape(name)),
            "",
            manifest,
            flags=re.MULTILINE,
        )
        manifest = manifest.replace('"%s", ' % name, "").replace(', "%s"' % name, "")

    return manifest, kept, dropped


def main(argv: list[str]) -> int:
    manifest_path, core_version = argv[1], argv[2]
    with open(manifest_path) as handle:
        manifest = handle.read()
    try:
        rewritten, kept, dropped = rewrite(manifest, published_features(core_version))
    except ManifestError as error:
        print("error: %s" % error, file=sys.stderr)
        return 1
    with open(manifest_path, "w") as handle:
        handle.write(rewritten)
    print(
        "proxy core %s: kept %s; dropped %s"
        % (core_version, ", ".join(kept) or "(none)", ", ".join(dropped) or "(none)")
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

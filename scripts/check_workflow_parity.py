#!/usr/bin/env python3
"""Fail when a release workflow tests a runtime version CI does not.

The v0.17.0 release shipped to crates.io, PyPI, and npm's wasm package while
the native npm package stalled: `publish-js.yml` still ran the ava suite on
Node 20 after ava 8 dropped it, and only `js.yml` had been given the Node 22+
gate. Nothing compared the two matrices, so the gap stayed invisible until the
tag already existed and half the registries were published.

This checker mechanizes that comparison for every runtime with both a CI
workflow and a release workflow:

* **Version coverage** - every runtime version a release workflow tests must
  also be tested by the CI workflow. A version CI stopped covering must not
  keep running at release time.
* **Suite gating** - for each test suite both workflows run, the versions it
  runs on at release time must be a subset of the versions CI runs it on. This
  is the Node 20 / ava failure, stated generally.
* **Toolchain pins** - every `dtolnay/rust-toolchain@<sha> # <label>` reference
  sharing a label must share a SHA across all workflows, and the stable label
  must match `rust-toolchain.toml`. A release workflow left behind on an older
  rustc is the same class of drift.

`if:` expressions are read only for comparisons against the job's matrix
version key (`matrix.node == '20'`, `matrix.version != '20'`). A version is
considered covered by a step when every such comparison holds; conditions on
anything else (`runner.os`, `github.event_name`) are ignored, which can only
make this checker stricter, never more permissive.
"""

from __future__ import annotations

import pathlib
import re
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github/workflows"
TOOLCHAIN = ROOT / "rust-toolchain.toml"

# Test suites worth comparing across workflows. A step belongs to a suite when
# its `run:` matches the pattern; steps matching nothing (installs, smoke
# one-liners, artifact downloads) are ignored.
SUITES = {
    "ava": re.compile(r"\bava\b|pnpm (?:run )?test\b"),
    "runtime-compat": re.compile(r"--test\s+[\"']?__test__/runtime-compat"),
    "pytest": re.compile(r"\bpytest\b"),
}

# (runtime, CI source, release sources). Each source names the workflow, the
# job, the matrix key holding the version, and an optional matrix row filter.
LANES = [
    {
        "runtime": "node",
        "ci": [("js.yml", "build-and-test", "version", {"runtime": "node"})],
        "release": [
            ("publish-js.yml", "test-js-macos-windows", "node", {}),
            ("publish-js.yml", "test-js-linux", "node", {}),
        ],
    },
    {
        "runtime": "python",
        "ci": [("python.yml", "test", "python-version", {})],
        "release": [("publish-python.yml", "test-builds", "python-version", {})],
    },
]

TOOLCHAIN_REF = re.compile(r"dtolnay/rust-toolchain@([0-9a-f]{40})\s*#\s*(\S+)")
MATRIX_COMPARISON = re.compile(r"matrix\.([\w-]+)\s*(==|!=)\s*'([^']*)'")


class ParityError(Exception):
    """A workflow pair drifted apart."""


def load(workflow: str) -> dict:
    return yaml.safe_load((WORKFLOWS / workflow).read_text())


def matrix_versions(job: dict, version_key: str, select: dict) -> list[str]:
    """Versions a job's matrix runs, honouring `include:` rows and a filter."""
    matrix = job.get("strategy", {}).get("matrix", {})
    versions = [str(v) for v in matrix.get(version_key, [])]
    for row in matrix.get("include", []):
        if version_key not in row:
            continue
        if any(str(row.get(k)) != v for k, v in select.items()):
            continue
        version = str(row[version_key])
        if version not in versions:
            versions.append(version)
    if select and not matrix.get("include"):
        # A filter with nothing to filter on means the lane is misconfigured.
        raise ParityError(f"matrix filter {select} matched no include rows")
    return versions


def step_applies(step: dict, version_key: str, version: str) -> bool:
    """Whether `step` runs for `version`, per its `if:` matrix comparisons."""
    condition = str(step.get("if", ""))
    for key, operator, literal in MATRIX_COMPARISON.findall(condition):
        if key != version_key:
            continue
        if operator == "==" and version != literal:
            return False
        if operator == "!=" and version == literal:
            return False
    return True


def suites_by_version(source: tuple[str, str, str, dict]) -> dict[str, set[str]]:
    """Map each matrix version to the test suites that run on it."""
    workflow_name, job_name, version_key, select = source
    workflow = load(workflow_name)
    job = workflow.get("jobs", {}).get(job_name)
    if job is None:
        raise ParityError(f"{workflow_name}: job '{job_name}' not found")

    coverage = {v: set() for v in matrix_versions(job, version_key, select)}
    if not coverage:
        raise ParityError(f"{workflow_name}:{job_name}: no matrix versions found")

    for step in job.get("steps", []):
        command = str(step.get("run", ""))
        if not command:
            continue
        tags = {tag for tag, pattern in SUITES.items() if pattern.search(command)}
        for version in coverage:
            if tags and step_applies(step, version_key, version):
                coverage[version] |= tags
    return coverage


def merge(sources: list[tuple[str, str, str, dict]]) -> dict[str, set[str]]:
    merged: dict[str, set[str]] = {}
    for source in sources:
        for version, tags in suites_by_version(source).items():
            merged.setdefault(version, set())
            merged[version] |= tags
    return merged


def check_lane(lane: dict) -> list[str]:
    runtime = lane["runtime"]
    ci = merge(lane["ci"])
    release = merge(lane["release"])
    errors = []

    for version in sorted(set(release) - set(ci)):
        errors.append(
            f"{runtime} {version}: tested by the release workflow but not by CI; "
            "add it to CI or drop it from the release matrix"
        )

    ci_suites = {tag for tags in ci.values() for tag in tags}
    for version, tags in sorted(release.items()):
        if version not in ci:
            continue
        for tag in sorted(tags & ci_suites - ci[version]):
            errors.append(
                f"{runtime} {version}: release workflow runs the '{tag}' suite, "
                f"CI does not run it on {version}; gate them the same way"
            )
    return errors


def check_toolchain_pins(
    workflows: pathlib.Path | None = None, toolchain: pathlib.Path | None = None
) -> list[str]:
    workflows = workflows or WORKFLOWS
    toolchain = toolchain or TOOLCHAIN
    pins: dict[str, dict[str, list[str]]] = {}
    for path in sorted(workflows.glob("*.yml")):
        for sha, label in TOOLCHAIN_REF.findall(path.read_text()):
            pins.setdefault(label, {}).setdefault(sha, []).append(path.name)

    errors = []
    for label, shas in sorted(pins.items()):
        if len(shas) > 1:
            where = "; ".join(
                f"{sha[:8]} in {', '.join(sorted(set(files)))}"
                for sha, files in sorted(shas.items())
            )
            errors.append(f"rust-toolchain '{label}' pinned to several SHAs: {where}")

    channel = re.search(r'channel\s*=\s*"([^"]+)"', toolchain.read_text())
    if channel and channel.group(1) not in pins and "nightly" != channel.group(1):
        errors.append(
            f"rust-toolchain.toml pins {channel.group(1)}, but no workflow "
            f"references that version (workflows use: {', '.join(sorted(pins))})"
        )
    return errors


def main() -> int:
    errors = []
    for lane in LANES:
        errors += check_lane(lane)
    errors += check_toolchain_pins()

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        print(
            f"\n{len(errors)} release/CI parity problem(s). "
            "A release workflow must not test what CI does not.",
            file=sys.stderr,
        )
        return 1

    lanes = ", ".join(lane["runtime"] for lane in LANES)
    print(f"release workflows match CI for: {lanes}; rust toolchain pins agree")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

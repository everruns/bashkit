#!/usr/bin/env python3
"""Validate the public capability contract and render its generated inventory."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "contracts/capability-parity.json"
OUTPUT_PATH = ROOT / "knowledge/status/capability-parity.md"
SURFACE_IDS = {
    "rust_bash",
    "rust_bash_tool",
    "rust_scripted_tool",
    "cli",
    "python",
    "napi",
    "browser_wasm",
    "c_abi",
}


class ContractError(ValueError):
    """The canonical contract is incomplete or references stale evidence."""


def load_manifest(path: Path = MANIFEST_PATH) -> dict[str, Any]:
    return json.loads(path.read_text())


def validate_manifest(manifest: dict[str, Any], root: Path = ROOT) -> None:
    if manifest.get("schema_version") != 1:
        raise ContractError("schema_version must be 1")
    capabilities = manifest.get("capabilities")
    surfaces = manifest.get("surfaces")
    if not isinstance(capabilities, dict) or not capabilities:
        raise ContractError("capabilities must be a non-empty object")
    if not isinstance(surfaces, dict) or set(surfaces) != SURFACE_IDS:
        raise ContractError(f"surfaces must be exactly {sorted(SURFACE_IDS)}")

    capability_ids = set(capabilities)
    for capability_id, description in capabilities.items():
        if not isinstance(description, str) or not description.strip():
            raise ContractError(f"{capability_id}: description must be non-empty")

    for surface_id, surface in surfaces.items():
        label = surface.get("label")
        supported = surface.get("supported")
        unsupported = surface.get("unsupported")
        evidence = surface.get("evidence")
        if not isinstance(label, str) or not label.strip():
            raise ContractError(f"{surface_id}: label must be non-empty")
        if not isinstance(supported, list) or len(supported) != len(set(supported)):
            raise ContractError(f"{surface_id}: supported must be a unique list")
        if not isinstance(unsupported, dict) or not isinstance(evidence, dict):
            raise ContractError(f"{surface_id}: unsupported/evidence must be objects")

        supported_ids = set(supported)
        unsupported_ids = set(unsupported)
        if supported_ids & unsupported_ids:
            overlap = sorted(supported_ids & unsupported_ids)
            raise ContractError(f"{surface_id}: duplicate status for {overlap}")
        if supported_ids | unsupported_ids != capability_ids:
            missing = sorted(capability_ids - supported_ids - unsupported_ids)
            extra = sorted((supported_ids | unsupported_ids) - capability_ids)
            raise ContractError(
                f"{surface_id}: capability coverage mismatch; missing={missing}, extra={extra}"
            )
        if set(evidence) != supported_ids:
            missing = sorted(supported_ids - set(evidence))
            extra = sorted(set(evidence) - supported_ids)
            raise ContractError(
                f"{surface_id}: evidence mismatch; missing={missing}, extra={extra}"
            )

        for capability_id, reason in unsupported.items():
            if not isinstance(reason, str) or not reason.strip():
                raise ContractError(
                    f"{surface_id}.{capability_id}: unsupported reason must be non-empty"
                )
        for capability_id, references in evidence.items():
            if not isinstance(references, list) or not references:
                raise ContractError(
                    f"{surface_id}.{capability_id}: evidence must be a non-empty list"
                )
            for reference in references:
                _validate_reference(surface_id, capability_id, reference, root)


def _validate_reference(
    surface_id: str, capability_id: str, reference: Any, root: Path
) -> None:
    if not isinstance(reference, str) or "#" not in reference:
        raise ContractError(
            f"{surface_id}.{capability_id}: evidence must be path#test-selector"
        )
    relative_path, selector = reference.split("#", 1)
    if not relative_path or not selector:
        raise ContractError(
            f"{surface_id}.{capability_id}: evidence must be path#test-selector"
        )
    relative = Path(relative_path)
    resolved_root = root.resolve()
    path = (resolved_root / relative).resolve()
    if relative.is_absolute() or ".." in relative.parts or not path.is_relative_to(
        resolved_root
    ):
        raise ContractError(
            f"{surface_id}.{capability_id}: evidence path must be repository-relative"
        )
    if not path.is_file():
        raise ContractError(f"{surface_id}.{capability_id}: missing evidence file {relative_path}")
    if selector not in path.read_text():
        raise ContractError(
            f"{surface_id}.{capability_id}: selector {selector!r} missing from {relative_path}"
        )


def render_inventory(manifest: dict[str, Any]) -> str:
    capabilities = manifest["capabilities"]
    surfaces = manifest["surfaces"]
    surface_ids = list(surfaces)
    lines = [
        "---",
        "type: Generated Inventory",
        "title: Public Capability Parity",
        "description: Generated support contract for capabilities across every public Bashkit surface.",
        "resource: ../../contracts/capability-parity.json",
        "tags:",
        "  - bashkit",
        "  - capabilities",
        "  - generated",
        "generated:",
        "  by: process:just-regen-capability-parity",
        "---",
        "",
        "# Public Capability Parity",
        "",
        "Generated from [`contracts/capability-parity.json`](../../contracts/capability-parity.json).",
        "Do not edit this file by hand. Run `just regen-capability-parity`.",
        "",
        "A check means the surface has an end-to-end test selector in the manifest.",
        "A dash means the feature is intentionally unsupported and has a recorded reason below.",
        "",
        "| Capability | " + " | ".join(surfaces[s]["label"] for s in surface_ids) + " |",
        "|---|" + "---|" * len(surface_ids),
    ]
    for capability_id, description in capabilities.items():
        cells = [
            "✅" if capability_id in surfaces[surface_id]["supported"] else "—"
            for surface_id in surface_ids
        ]
        lines.append(f"| {description} | " + " | ".join(cells) + " |")

    lines.extend(["", "## Intentional exclusions", ""])
    for surface_id in surface_ids:
        surface = surfaces[surface_id]
        lines.extend([f"### {surface['label']}", ""])
        for capability_id, reason in surface["unsupported"].items():
            lines.append(f"- `{capability_id}`: {reason}")
        lines.append("")

    lines.extend(
        [
            "## Enforcement",
            "",
            "`just check-capability-parity` validates complete matrix coverage, non-empty",
            "unsupported reasons, and every supported capability's `path#test-selector` evidence.",
            "The language/package test jobs execute those selectors; deleting or renaming evidence",
            "breaks the central check instead of silently weakening the contract.",
            "",
            "## See also",
            "",
            "- [Testing Strategy](../operations/testing.md) — where each surface's executable tests run",
            "- [Threat Model](../security/threat-model.md) — security impact of silently dropped wrapper configuration",
            "- [Bashkit Architecture](../foundations/architecture.md) — public surface ownership",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail when generated output drifts")
    args = parser.parse_args()
    manifest = load_manifest()
    validate_manifest(manifest)
    expected = render_inventory(manifest)
    if args.check:
        actual = OUTPUT_PATH.read_text() if OUTPUT_PATH.is_file() else ""
        if actual != expected:
            raise SystemExit(
                "capability parity inventory is stale; run just regen-capability-parity"
            )
    else:
        OUTPUT_PATH.write_text(expected)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

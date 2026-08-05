import unittest
from copy import deepcopy
from pathlib import Path

from scripts.capability_parity import (
    ContractError,
    OUTPUT_PATH,
    load_manifest,
    render_inventory,
    validate_manifest,
)


ROOT = Path(__file__).resolve().parents[2]


class CapabilityParityContractTests(unittest.TestCase):
    def test_canonical_manifest_exists(self) -> None:
        self.assertTrue((ROOT / "contracts/capability-parity.json").is_file())

    def test_every_surface_declares_every_capability_with_live_evidence(self) -> None:
        validate_manifest(load_manifest())

    def test_generated_inventory_is_current(self) -> None:
        self.assertEqual(OUTPUT_PATH.read_text(), render_inventory(load_manifest()))

    def test_missing_surface_cell_is_rejected(self) -> None:
        manifest = deepcopy(load_manifest())
        manifest["surfaces"]["c_abi"]["unsupported"].pop("snapshots")
        with self.assertRaisesRegex(ContractError, "coverage mismatch"):
            validate_manifest(manifest)

    def test_stale_evidence_selector_is_rejected(self) -> None:
        manifest = deepcopy(load_manifest())
        manifest["surfaces"]["c_abi"]["evidence"]["cwd"] = [
            "crates/bashkit-capi/tests/abi.rs#missing_contract_test"
        ]
        with self.assertRaisesRegex(ContractError, "selector .* missing"):
            validate_manifest(manifest)

    def test_evidence_must_stay_inside_repository(self) -> None:
        manifest = deepcopy(load_manifest())
        manifest["surfaces"]["c_abi"]["evidence"]["cwd"] = ["/etc/passwd#root"]
        with self.assertRaisesRegex(ContractError, "repository-relative"):
            validate_manifest(manifest)


if __name__ == "__main__":
    unittest.main()

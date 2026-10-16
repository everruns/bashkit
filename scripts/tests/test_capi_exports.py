"""The shipped Windows import library must expose the complete public C ABI."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2] / "crates/bashkit-capi"


class CApiExportsTests(unittest.TestCase):
    def test_header_rust_and_windows_exports_match(self):
        header = re.sub(r"/\*.*?\*/|//[^\n]*", "", (ROOT / "include/bashkit.h").read_text(), flags=re.S)
        declared = set(re.findall(r"\b(bashkit_\w+)\s*\(", header))
        windows = set(re.findall(r"^\s+(bashkit_\w+)\s*$", (ROOT / "include/bashkit.def").read_text(), re.M))
        rust = set()
        for source in (ROOT / "src").rglob("*.rs"):
            rust.update(re.findall(r'#\[unsafe\(no_mangle\)\]\s+pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(bashkit_\w+)', source.read_text()))
        self.assertTrue(declared)
        self.assertEqual(declared, rust, "public header and Rust entry points differ")
        self.assertEqual(declared, windows, "Windows SDK import-library exports differ from the public header")


if __name__ == "__main__":
    unittest.main()

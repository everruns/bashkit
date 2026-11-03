"""Compile the real CPython embedding without Wasmtime or the large artifact.

COFF cannot encode a 64 KiB-aligned static. Probe the production embedding
with a tiny payload so Windows object generation stays a cheap regression test.
On ELF, also select the Windows cfg to exercise the plain-slice allocation path.
"""

from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/bashkit-cpython-wasm/src/lib.rs"

PROBE = """
fn main() {
    #[cfg(any(windows, target_vendor = "apple"))]
    let embedded: &[u8] = CWASM;
    #[cfg(not(any(windows, target_vendor = "apple")))]
    let embedded: &[u8] = &CWASM.0;
    let bytes = cwasm_bytes();
    assert_eq!(bytes, embedded);
    assert_eq!(bytes.as_ptr() as usize % 65536, 0);
    assert_eq!(bytes.as_ptr(), cwasm_bytes().as_ptr());
    for thread in (0..4).map(|_| std::thread::spawn(cwasm_bytes)).collect::<Vec<_>>() {
        assert_eq!(bytes.as_ptr(), thread.join().unwrap().as_ptr());
    }
}
"""


@unittest.skipUnless(shutil.which("rustc"), "rustc required for embedding probe")
class CPythonEmbeddingTests(unittest.TestCase):
    def probe(self, cfg: list[str]) -> None:
        source = SOURCE.read_text()
        # Use the actual statics, padded allocation and accessor, without the
        # Wasmtime API/build script. Changes to their cfg must reach this probe.
        embedding = source[source.index("const MODULE_ALIGNMENT"):source.index("/// Create an engine")]
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "python.cwasm").write_bytes(bytes(range(64)))
            probe = root / "probe.rs"
            probe.write_text(embedding + PROBE)
            env = dict(os.environ, OUT_DIR=str(root))
            command = ["rustc", "--edition=2024", str(probe), *cfg]
            # Object generation is the original compiler-crash reproducer.
            for args in (["--emit=obj", "-o", str(root / "probe.obj")],
                         ["-o", str(root / "probe.exe")]):
                result = subprocess.run(command + args, env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            subprocess.run([str(root / "probe.exe")], check=True)

    def test_host_embedding(self) -> None:
        self.probe([])

    @unittest.skipIf(sys.platform in ("win32", "darwin"), "host already uses the plain slice")
    def test_windows_plain_slice_on_elf(self) -> None:
        self.probe(["--cfg", "windows", "-Aexplicit_builtin_cfgs_in_flags"])


if __name__ == "__main__":
    unittest.main()

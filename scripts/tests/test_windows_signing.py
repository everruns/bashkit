"""Exercise wheel RECORD repair and real Windows Authenticode validation."""

import base64
import csv
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile

import yaml

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("windows_wheel", ROOT / "scripts/windows_wheel.py")
wheel_helper = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(wheel_helper)


def make_wheel(directory, *, corrupt=False, native=True):
    files = {
        "bashkit/__init__.py": b"# package\n",
        "bashkit-1.2.3.dist-info/METADATA": b"Metadata-Version: 2.1\nName: bashkit\nVersion: 1.2.3\n",
        "bashkit-1.2.3.dist-info/WHEEL": (
            b"Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: cp39-abi3-win_amd64\n"
        ),
    }
    if native:
        files["bashkit/_bashkit.pyd"] = b"MZunsigned test module"
    record = io.StringIO(newline="")
    writer = csv.writer(record)
    for name, content in files.items():
        digest = base64.urlsafe_b64encode(hashlib.sha256(content).digest()).rstrip(b"=").decode()
        writer.writerow((name, "sha256=" + digest, len(content)))
    record_name = "bashkit-1.2.3.dist-info/RECORD"
    writer.writerow((record_name, "", ""))
    files[record_name] = record.getvalue().encode()
    if corrupt:
        files["bashkit/__init__.py"] += b"# modified without updating RECORD\n"
    path = directory / "bashkit-1.2.3-cp39-abi3-win_amd64.whl"
    with zipfile.ZipFile(path, "w") as archive:
        for name, content in files.items():
            archive.writestr(name, content)
    return path


@unittest.skipUnless(importlib.util.find_spec("wheel"), "requires wheel; installed by Windows signing CI")
class WheelSigningTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.source = self.base / "source"
        self.source.mkdir()
        self.work = self.base / "work"
        self.output = self.base / "signed"

    def test_native_change_regenerates_record_and_preserves_other_contents(self):
        original = make_wheel(self.source)
        package = wheel_helper.prepare(self.source, self.work)
        native = package / "bashkit/_bashkit.pyd"
        signed = native.read_bytes() + b"test signature bytes"
        native.write_bytes(signed)
        result = wheel_helper.repack(self.work, self.output)
        self.assertEqual(result.name, original.name)
        with zipfile.ZipFile(result) as archive:
            self.assertEqual(archive.read("bashkit/_bashkit.pyd"), signed)
            self.assertEqual(archive.read("bashkit/__init__.py"), b"# package\n")
            rows = dict((row[0], row[1:]) for row in csv.reader(io.StringIO(
                archive.read("bashkit-1.2.3.dist-info/RECORD").decode()
            )))
        expected = base64.urlsafe_b64encode(hashlib.sha256(signed).digest()).rstrip(b"=").decode()
        self.assertEqual(rows["bashkit/_bashkit.pyd"], ["sha256=" + expected, str(len(signed))])

    def test_rejects_corrupt_unsigned_wheel(self):
        make_wheel(self.source, corrupt=True)
        with self.assertRaises(subprocess.CalledProcessError):
            wheel_helper.prepare(self.source, self.work)

    def test_rejects_non_native_changes_and_file_additions_or_removals(self):
        for operation in ("modify", "add", "remove"):
            with self.subTest(operation=operation), tempfile.TemporaryDirectory() as temp:
                base = Path(temp)
                source = base / "source"
                source.mkdir()
                make_wheel(source)
                package = wheel_helper.prepare(source, base / "work")
                if operation == "modify":
                    (package / "bashkit/__init__.py").write_text("changed")
                elif operation == "add":
                    (package / "bashkit/extra.dll").write_bytes(b"extra")
                else:
                    (package / "bashkit/_bashkit.pyd").unlink()
                with self.assertRaises(ValueError):
                    wheel_helper.repack(base / "work", base / "signed")

    def test_rejects_missing_native_module_and_existing_work_directory(self):
        make_wheel(self.source, native=False)
        with self.assertRaises(ValueError):
            wheel_helper.prepare(self.source, self.work)
        with self.assertRaises(FileExistsError):
            wheel_helper.prepare(self.source, self.work)

    def test_rejects_missing_or_multiple_wheels(self):
        with self.assertRaises(ValueError):
            wheel_helper.prepare(self.source, self.work)
        path = make_wheel(self.source)
        shutil.copyfile(path, self.source / path.name.replace("1.2.3", "1.2.4"))
        with self.assertRaises(ValueError):
            wheel_helper.prepare(self.source, self.work)


@unittest.skipUnless(os.name == "nt", "requires Windows WinVerifyTrust")
class AuthenticodeTests(unittest.TestCase):
    def verify(self, root):
        return subprocess.run([
            "pwsh", "-NoProfile", "-File", str(ROOT / "scripts/verify_windows_signatures.ps1"),
            "-Root", str(root),
        ], capture_output=True, text=True)

    def test_accepts_timestamped_python_signature_on_pe_bytes(self):
        # Hosted setup-python uses the signed python.org distribution. Renaming
        # a copy to .pyd checks the same PE verifier used for our native module.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            shutil.copyfile(sys.executable, root / "_bashkit.pyd")
            result = self.verify(root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_rejects_tampering_unsigned_files_and_empty_directory(self):
        for case in ("empty", "unsigned", "tampered", "unsigned_dependency"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                if case == "unsigned":
                    (root / "_bashkit.pyd").write_bytes(b"MZunsigned")
                elif case in ("tampered", "unsigned_dependency"):
                    native = root / "_bashkit.pyd"
                    shutil.copyfile(sys.executable, native)
                    if case == "tampered":
                        content = bytearray(native.read_bytes())
                        content[1024] ^= 1
                        native.write_bytes(content)
                    else:
                        (root / "dependency.dll").write_bytes(b"MZunsigned")
                self.assertNotEqual(self.verify(root).returncode, 0)


class SigningWorkflowTests(unittest.TestCase):
    def test_unsigned_artifacts_cannot_reach_pypi(self):
        jobs = yaml.safe_load((ROOT / ".github/workflows/publish-python.yml").read_text())["jobs"]
        self.assertIn("sign-windows", jobs["publish"]["needs"])
        self.assertIn("sign-windows", jobs["test-builds"]["needs"])
        signing = jobs["sign-windows"]
        self.assertIn("inspect", signing["needs"])
        self.assertEqual(signing["environment"], "release-windows-signing")
        self.assertEqual(signing["permissions"], {"contents": "read", "id-token": "write"})
        upload = next(s for s in jobs["build"]["steps"] if s.get("uses", "").startswith("actions/upload-artifact@"))
        self.assertIn("pypi_unsigned", upload["with"]["name"])
        self.assertIn("matrix.os == 'windows'", upload["with"]["name"])
        sign_step = next(s for s in signing["steps"] if s.get("uses", "").startswith("azure/artifact-signing-action@"))
        self.assertEqual(sign_step["with"]["files-folder-filter"], "pyd,dll,exe")
        self.assertEqual(sign_step["with"]["timestamp-digest"], "SHA256")
        publish_downloads = [s for s in jobs["publish"]["steps"]
                             if s.get("uses", "").startswith("actions/download-artifact@")]
        self.assertTrue(publish_downloads)
        for step in publish_downloads:
            self.assertEqual(step["with"]["pattern"], "pypi_files-*")
        signed_upload = next(s for s in signing["steps"]
                             if s.get("uses", "").startswith("actions/upload-artifact@"))
        self.assertEqual(signed_upload["with"]["path"], "signed-dist")
        self.assertEqual(signed_upload["with"]["name"], "pypi_files-windows-x86_64-manylinux")
        for name in ("build", "build-sdist", "build-emscripten", "test-builds"):
            self.assertNotEqual(jobs[name].get("permissions", {}).get("id-token"), "write")


if __name__ == "__main__":
    unittest.main()

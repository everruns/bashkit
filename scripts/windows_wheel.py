#!/usr/bin/env python3
"""Sign only native files between wheel unpack/pack; never reuse stale RECORDs.

The pinned wheel CLI owns the wheel format and integrity checks. This helper
additionally rejects changes to Python/metadata files and isolates the signed
archive from unsigned build outputs. It never imports the packaged extension.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

NATIVE_SUFFIXES = {".pyd", ".dll", ".exe"}


def _files(root):
    result = {}
    for path in root.rglob("*"):
        if path.is_symlink():
            raise ValueError("wheel contains a symbolic link")
        if path.is_file():
            result[path.relative_to(root).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def prepare(source, work):
    wheels = list(source.glob("*.whl"))
    if len(wheels) != 1 or not wheels[0].name.startswith("bashkit-") or not wheels[0].name.endswith("-win_amd64.whl"):
        raise ValueError("expected exactly one Bashkit Windows x64 wheel")
    work.mkdir(parents=True, exist_ok=False)
    subprocess.run([
        sys.executable, "-m", "wheel", "unpack", str(wheels[0].resolve()), "-d", str(work.resolve()),
    ], check=True)
    roots = [path for path in work.iterdir() if path.is_dir()]
    if len(roots) != 1 or not (roots[0] / "bashkit/_bashkit.pyd").is_file():
        raise ValueError("wheel must contain bashkit/_bashkit.pyd")
    root = roots[0]
    (work / "unsigned-manifest.json").write_text(json.dumps({
        "root": root.name, "wheel": wheels[0].name, "files": _files(root),
    }), encoding="utf-8")
    return root.resolve()


def repack(work, output):
    manifest = json.loads((work / "unsigned-manifest.json").read_text(encoding="utf-8"))
    root = work / manifest["root"]
    if root.resolve().parent != work.resolve():
        raise ValueError("invalid wheel root")
    current = _files(root)
    original = manifest["files"]
    if current.keys() != original.keys():
        raise ValueError("signing added or removed wheel files")
    for name, digest in original.items():
        if Path(name).suffix.lower() not in NATIVE_SUFFIXES and current[name] != digest:
            raise ValueError("signing changed a non-native file: " + name)
    output.mkdir(parents=True, exist_ok=False)
    subprocess.run([
        sys.executable, "-m", "wheel", "pack", str(root.resolve()), "-d", str(output.resolve()),
    ], check=True)
    wheels = list(output.glob("*.whl"))
    if len(wheels) != 1 or wheels[0].name != manifest["wheel"]:
        raise ValueError("repacking changed the wheel identity")
    # Read every member through WheelFile so corrupt/stale RECORD hashes fail
    # before this directory can become a publishable artifact.
    from wheel.wheelfile import WheelFile
    with WheelFile(str(wheels[0])) as archive:
        for member in archive.infolist():
            if not member.is_dir():
                archive.read(member.filename)
    return wheels[0].resolve()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    unpack = commands.add_parser("prepare")
    unpack.add_argument("--source", type=Path, required=True)
    unpack.add_argument("--work", type=Path, required=True)
    pack = commands.add_parser("repack")
    pack.add_argument("--work", type=Path, required=True)
    pack.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        name, value = "root", prepare(args.source, args.work)
    else:
        name, value = "wheel", repack(args.work, args.output)
    print(json.dumps({name: str(value)}))
    if os.environ.get("GITHUB_OUTPUT"):
        if "\n" in str(value) or "\r" in str(value):
            raise ValueError("invalid action output path")
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(f"{name}={value}\n")


if __name__ == "__main__":
    main()

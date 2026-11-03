"""Publish retained bytes; recover a staged conflict only after public proof.

A staged E409 can follow a successful upload. Never approve private stages or
accept version equality alone: require the retained archive hash and dist-tag.
"""

import argparse
import base64
import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path


def run_command(args):
    return subprocess.run(
        args,
        check=False,
        capture_output=True,
        text=True,
        timeout=600 if args[1] == "publish" else 30,
    )


def publish(
    tag, *, run=run_command, sleep=time.sleep, now=time.monotonic, attempts=60, delay=15
):
    manifest = json.loads(Path("package.json").read_text())
    packed = run(["npm", "pack", "--ignore-scripts", "--json"])
    if packed.returncode:
        raise RuntimeError("npm pack failed: " + packed.stderr)
    package = json.loads(packed.stdout)[0]
    archive = Path(package["filename"])
    digest = hashlib.sha512()
    with archive.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    integrity = "sha512-" + base64.b64encode(digest.digest()).decode()
    if integrity != package["integrity"]:
        raise RuntimeError("npm pack archive integrity mismatch")
    result = run(
        [
            "npm",
            "publish",
            str(archive),
            "--ignore-scripts",
            "--provenance",
            "--access",
            "public",
            "--tag",
            tag,
        ]
    )
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    output = result.stdout + result.stderr
    staged = (
        "E409" in output and "Cannot publish over previously staged version" in output
    )
    if result.returncode and not staged:
        raise RuntimeError("npm publish failed")

    def read_public(args):
        try:
            response = run(args)
            return json.loads(response.stdout) if response.returncode == 0 else None
        except (ValueError, subprocess.TimeoutExpired):
            return None

    deadline = now() + attempts * delay
    for attempt in range(attempts):
        if now() >= deadline:
            break
        published = read_public(
            [
                "npm",
                "view",
                f"{manifest['name']}@{manifest['version']}",
                "dist.integrity",
                "--json",
            ]
        )
        if published is not None:
            if published != integrity:
                raise RuntimeError("published npm archive integrity mismatch")
            current = read_public(
                ["npm", "view", manifest["name"], f"dist-tags.{tag}", "--json"]
            )
            if current == manifest["version"]:
                print(f"Verified {manifest['name']}@{current} ({tag}): {integrity}")
                return
        if attempt + 1 < attempts:
            sleep(min(delay, max(0, deadline - now())))
    raise RuntimeError(
        "npm publication not verified: exact public archive and dist-tag required"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", choices=("latest", "next"), required=True)
    args = parser.parse_args()
    try:
        publish(args.tag)
    except (RuntimeError, ValueError, OSError, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

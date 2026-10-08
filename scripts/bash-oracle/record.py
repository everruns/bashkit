#!/usr/bin/env python3
"""Re-record the expected output of the shell-parity cases from real bash.

Run on a machine with the bash version README.md names, after adding or
changing a case. A case is only recorded when two runs in different empty
directories agree and the output carries neither a host path nor the working
directory, so the corpus stays reproducible on another machine.

A case whose output names the working directory is dropped rather than
recorded: bashkit's virtual filesystem starts in `/home/user` whatever the
process cwd is, by design, so such a case asks about sandbox identity rather
than about bash parity.
"""

import os
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
# Bash runs with bashkit's own sandbox identity (HOME, USER), so a gap in the
# corpus is a behavioral difference rather than a difference in who the shell
# thinks it is. Locale is pinned so locale-sensitive tools agree.
ENV = {
    "LC_ALL": "C.UTF-8",
    "HOME": "/home/sandbox",
    "USER": "sandbox",
    "TERM": "dumb",
}


def run(case: pathlib.Path) -> tuple[str, str, int]:
    with tempfile.TemporaryDirectory() as work:
        env = dict(ENV, PATH=os.environ["PATH"])
        proc = subprocess.run(
            ["bash", str(case)],
            cwd=work,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=60,
            env=env,
        )
        out = proc.stdout.decode("utf-8", "replace").replace(str(case), "SCRIPT")
        err = proc.stderr.decode("utf-8", "replace").replace(str(case), "SCRIPT")
        return out.replace(work, "CWD"), err.replace(work, "CWD"), proc.returncode


def main() -> int:
    kept, dropped = 0, []
    for case in sorted((HERE / "cases").glob("*.sh")):
        try:
            first, second = run(case), run(case)
        except subprocess.TimeoutExpired:
            dropped.append((case.stem, "times out under bash"))
            continue
        if first != second:
            dropped.append((case.stem, "output differs between runs"))
            continue
        out, err, status = first
        if "/tmp/" in out + err or str(HERE.parent.parent) in out + err:
            dropped.append((case.stem, "output carries a host path"))
            continue
        if "CWD" in out + err:
            dropped.append((case.stem, "output names the working directory"))
            continue
        (HERE / "expected" / f"{case.stem}.txt").write_text(
            f"### status: {status}\n### stdout\n{out}### stderr\n{err}"
        )
        kept += 1
    print(f"bash-oracle: recorded {kept} cases")
    for name, why in dropped:
        print(f"  skipped {name}: {why}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

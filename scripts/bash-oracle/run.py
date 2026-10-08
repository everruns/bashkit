#!/usr/bin/env python3
"""Shell-parity scoreboard: every recorded case through bashkit, diffed with bash.

The recordings in `expected/` are the oracle, so a run needs no bash and does
not drift with the runner's bash version. `record.py` makes them, on a machine
with the bash the recordings name (see README.md).

Two scores, because they move at different speeds. `match` is status plus
stdout: what a script observes, and the number the roadmap tracks. `strict`
also requires the stderr text, which still carries bash wording bashkit does
not reproduce (parser error texts, xtrace details), so it trails behind.

`floor.txt` holds both floors, `<match> <strict>`. Exits 1 when either score
drops below its floor, so a regression fails CI; it also says when a floor is
behind, since a floor that lags stops catching anything.
"""

import argparse
import os
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
# Bash runs with bashkit's own sandbox identity (HOME, USER), so a gap in the
# corpus is a behavioral difference rather than a difference in who the shell
# thinks it is. Locale is pinned so locale-sensitive tools agree.
ENV = {
    "LC_ALL": "C.UTF-8",
    "HOME": "/home/sandbox",
    "USER": "sandbox",
    "TERM": "dumb",
}


def parse(text: str) -> dict:
    """The three sections of a recording, keyed by name."""
    out, name = {}, None
    for line in text.splitlines(keepends=True):
        if line.startswith("### status: "):
            out["status"] = line[len("### status: ") :].strip()
            name = None
        elif line in ("### stdout\n", "### stderr\n"):
            name = line[4:].strip()
            out[name] = ""
        elif name is not None:
            out[name] += line
    return out


def run_case(binary: str, case: pathlib.Path) -> str:
    with tempfile.TemporaryDirectory() as work:
        env = dict(ENV, PATH=os.environ["PATH"])
        proc = subprocess.run(
            [binary, str(case)],
            cwd=work,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=60,
            env=env,
        )
        out = proc.stdout.decode("utf-8", "replace").replace(str(case), "SCRIPT")
        err = proc.stderr.decode("utf-8", "replace").replace(str(case), "SCRIPT")
        out = out.replace(work, "CWD")
        err = err.replace(work, "CWD")
    return f"### status: {proc.returncode}\n### stdout\n{out}### stderr\n{err}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", default=os.environ.get("BASHKIT_BIN"))
    parser.add_argument("--case", help="run one case and print the diff")
    args = parser.parse_args()

    binary = args.bin or str(ROOT / "target" / "debug" / "bashkit")
    if not os.access(binary, os.X_OK):
        print(f"bash-oracle: no bashkit binary at {binary}", file=sys.stderr)
        print("bash-oracle: cargo build -p bashkit-cli", file=sys.stderr)
        return 2

    cases = sorted((HERE / "cases").glob("*.sh"))
    if args.case:
        cases = [c for c in cases if c.stem == args.case]
        if not cases:
            print(f"bash-oracle: no case named {args.case}", file=sys.stderr)
            return 2

    matched, strict, gaps, stderr_only = 0, 0, [], []
    for case in cases:
        expected_path = HERE / "expected" / f"{case.stem}.txt"
        if not expected_path.exists():
            continue
        expected = expected_path.read_text()
        try:
            actual = run_case(binary, case)
        except subprocess.TimeoutExpired:
            actual = "### status: timeout\n### stdout\n### stderr\n"
        want, got = parse(expected), parse(actual)
        observable = all(want.get(k) == got.get(k) for k in ("status", "stdout"))
        if observable:
            matched += 1
            if want.get("stderr") == got.get("stderr"):
                strict += 1
            else:
                stderr_only.append(case.stem)
        else:
            gaps.append(case.stem)
        if args.case and (not observable or want.get("stderr") != got.get("stderr")):
            for name in ("status", "stdout", "stderr"):
                if want.get(name, "") != got.get(name, ""):
                    print(
                        f"--- {name}\nbash:    {want.get(name, '')!r}"
                        f"\nbashkit: {got.get(name, '')!r}"
                    )

    total = matched + len(gaps)
    print(f"bash-oracle: {matched}/{total} cases match bash on status + stdout")
    print(f"bash-oracle: {strict}/{total} also match bash stderr")
    for name in gaps:
        print(f"  gap: {name}")
    for name in stderr_only:
        print(f"  stderr-only gap: {name}")

    floors = HERE / "floor.txt"
    match_floor, strict_floor = (int(n) for n in floors.read_text().split()[:2])
    failed = False
    for label, score, floor in (
        ("match", matched, match_floor),
        ("strict", strict, strict_floor),
    ):
        if score < floor:
            print(
                f"bash-oracle: {label} score {score} is below the floor of {floor}",
                file=sys.stderr,
            )
            failed = True
        elif score > floor:
            print(
                f"bash-oracle: {label} floor is {floor}; raise it to {score} "
                f"in scripts/bash-oracle/floor.txt"
            )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

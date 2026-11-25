"""Build matched baseline/candidate Bash adapters and alternate full event-JSON runs."""

import argparse
import hashlib
import json
import os
import shutil
import statistics
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--benchmark", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    here = Path(__file__).resolve().parent
    shutil.copytree(args.benchmark / "benches/rust-hosted/common", output / "common")
    env = os.environ.copy()
    env.pop("BENCH_QUICK", None)
    env["CARGO_TARGET_DIR"] = str(output / "target")
    binaries = {}
    for name, checkout in [("baseline", args.baseline), ("candidate", args.candidate)]:
        project = output / name
        (project / "src").mkdir(parents=True)
        shutil.copy2(here / "original-adapter.rs", project / "src/main.rs")
        shutil.copy2(here / "original-adapter.lock", project / "Cargo.lock")
        core = json.dumps(str(checkout.resolve() / "crates/bashkit"))
        (project / "Cargo.toml").write_text(
            '[package]\nname="bench-bashkit"\nversion="0.1.0"\n'
            'edition="2021"\npublish=false\n[workspace]\n[dependencies]\n'
            'bench-common={path="../common"}\n'
            f'bashkit={{path={core},features=["experimental_execution_plan"]}}\n'
            'tokio={version="1",features=["rt","time"]}\n'
            '[profile.release]\nopt-level=3\nlto="fat"\n'
            "codegen-units=1\ndebug=false\n"
        )
        with (output / f"build-{name}.log").open("w") as log:
            subprocess.run(
                ["cargo", "+1.100.0", "build", "--locked", "--release"],
                cwd=project,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=True,
            )
        binaries[name] = output / f"bench-{name}"
        shutil.copy2(output / "target/release/bench-bashkit", binaries[name])
    rows = []
    for round_number in range(4):
        order = ["baseline", "candidate"]
        if round_number % 2:
            order.reverse()
        for name in order:
            run = subprocess.run(
                [str(binaries[name]), "event-json"],
                cwd=output,
                env=env,
                text=True,
                capture_output=True,
                timeout=180,
                check=False,
            )
            (output / f"{round_number}-{name}.stdout").write_text(run.stdout)
            (output / f"{round_number}-{name}.stderr").write_text(run.stderr)
            run.check_returncode()
            data = json.loads(run.stdout)
            assert data["toolCallMeasuredCalls"] >= 100_000
            assert data["memorySessions"] == 1_000 and data["startRuns"] == 2_000
            rows.append({"round": round_number, "label": name, "output": data})
            print(name, round_number, data["toolCallWallUs"], flush=True)
            (output / "partial.json").write_text(json.dumps(rows, indent=2) + "\n")
    summary = {
        name: statistics.median(
            row["output"]["toolCallWallUs"] for row in rows if row["label"] == name
        )
        for name in binaries
    }
    result = {
        "profile": {"opt_level": 3, "lto": "fat", "codegen_units": 1},
        "lock_sha256": hashlib.sha256(
            (here / "original-adapter.lock").read_bytes()
        ).hexdigest(),
        "summary_us": summary,
        "runs": rows,
    }
    (output / "results.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()

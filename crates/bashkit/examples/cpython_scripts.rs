//! CPython Scripts Example
//!
//! Runs real CPython 3.14 (compiled to WebAssembly) as `python3` inside
//! Bashkit. The interpreter only sees the virtual filesystem and captured
//! stdio; every call starts from a fresh pre-initialized snapshot.
//!
//! Run with: cargo run --features cpython --example cpython_scripts

use bashkit::{Bash, CPython, CPythonLimits};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Optional: load the interpreter at startup instead of on the first call.
    CPython::warm_up().map_err(anyhow::Error::msg)?;

    let mut bash = Bash::builder()
        .cpython_with_limits(CPythonLimits::default().max_duration(Duration::from_secs(10)))
        .build();

    // Classes, dataclasses and the stdlib work as in regular CPython.
    let r = bash
        .exec(
            r#"python3 - <<'EOF'
from dataclasses import dataclass
import json, statistics

@dataclass
class Sample:
    name: str
    values: list

s = Sample("latency", [3, 1, 4, 1, 5])
print(json.dumps({"name": s.name, "median": statistics.median(s.values)}))
EOF"#,
        )
        .await?;
    print!("{}", r.stdout);
    assert_eq!(r.stdout, "{\"name\": \"latency\", \"median\": 3}\n");

    // Files written by Python are visible to the shell, and vice versa.
    let r = bash
        .exec(
            "printf 'a,b\\n1,2\\n3,4\\n' > /data.csv
             python3 -c 'import csv; rows = list(csv.DictReader(open(\"/data.csv\"))); \
               open(\"/sum.txt\", \"w\").write(str(sum(int(r[\"b\"]) for r in rows)))'
             cat /sum.txt",
        )
        .await?;
    println!("{}", r.stdout);
    assert_eq!(r.stdout, "6");

    // Exit codes and stderr behave like a real process.
    let r = bash
        .exec("python3 -c 'import sys; sys.exit(\"failed\")' || echo \"exit=$?\"")
        .await?;
    print!("{}", r.stdout);
    assert_eq!(r.stdout, "exit=1\n");

    // No network or subprocesses inside the sandbox.
    let r = bash
        .exec("python3 -c 'import subprocess; subprocess.run([\"ls\"])' 2>&1 | tail -1")
        .await?;
    print!("{}", r.stdout);
    assert!(r.stdout.contains("OSError"));
    Ok(())
}

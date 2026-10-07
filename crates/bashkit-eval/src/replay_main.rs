// bashkit-replay: replay recorded eval sessions on the current bashkit and
// write gap telemetry (see `bashkit_eval::replay` and
// knowledge/operations/eval.md#gap-telemetry).
//
//     cargo run -p bashkit-eval --bin bashkit-replay            # save report
//     cargo run -p bashkit-eval --bin bashkit-replay -- --print # stdout only

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use bashkit_eval::replay;

#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut results = root.join("results");
    let mut dataset = root.join("data/eval-tasks.jsonl");
    let mut out = root.join("results/gaps");
    let mut print = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--results" => results = args.next().context("--results needs a dir")?.into(),
            "--dataset" => dataset = args.next().context("--dataset needs a file")?.into(),
            "--out" => out = args.next().context("--out needs a dir")?.into(),
            "--print" => print = true,
            "-h" | "--help" => {
                println!(
                    "usage: bashkit-replay [--results DIR] [--dataset FILE] [--out DIR] [--print]"
                );
                return Ok(());
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    let sessions = replay::load_corpus(&results, &dataset)?;
    let mut replays = Vec::with_capacity(sessions.len());
    for s in &sessions {
        replays.push(replay::replay(s).await);
    }
    let now = chrono::Utc::now();
    let report = replay::build_report(
        &sessions,
        &replays,
        now.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    );
    let md = replay::render_markdown(&report);
    if print {
        print!("{md}");
        return Ok(());
    }
    std::fs::create_dir_all(&out)?;
    let stem = format!("gaps-{}", now.format("%Y%m%dT%H%M%SZ"));
    std::fs::write(
        out.join(format!("{stem}.json")),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    std::fs::write(out.join(format!("{stem}.md")), md)?;
    eprintln!("wrote {}/{stem}.{{json,md}}", out.display());
    Ok(())
}

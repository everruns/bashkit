//! Runs the pseudo-linus Debian-oracle cases against bashkit.
//!
//! Each case's fixture goes into bashkit's in-memory filesystem at
//! `harness::CASE_DIR` (modes, symlinks, mtimes), the script runs with the
//! bench environment, stdin and `ExecutionLimits::cli()`, and the outcome
//! (stdout, stderr, exit, final tree) is compared with the Debian golden by
//! the bench's own `harness::compare_outcome`.
//!
//! Output: one `tool<TAB>cases<TAB>strict<TAB>lenient` line per tool on
//! stdout; failing case ids with a short diff go to `$FAILS_OUT`.
//! Decision: each case runs on its own thread with a 10 s wall clock, so a
//! hang or panic costs that case only.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use bashkit::{Bash, ExecOptions, ExecutionLimits, FileSystem, FileType, GitConfig};
use harness::{Bytes, Entry, Invocation, MemTree, Outcome};

const CASE_TIMEOUT: Duration = Duration::from_secs(10);

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tools = if args.is_empty() { harness::paths::tools()? } else { args };
    let mut fails = Vec::new();
    for tool in tools {
        let (cases, _) = harness::paths::load_tool(&tool)?;
        let (mut strict, mut lenient) = (0, 0);
        for (case, golden) in &cases {
            let Ok(inv) = case.invocation() else { continue };
            let actual = run_isolated(inv);
            let cmp = harness::compare_outcome(case, golden, &actual);
            strict += usize::from(cmp.strict);
            if cmp.lenient {
                lenient += 1;
            } else {
                let detail: String = cmp.detail.join(" | ").chars().take(400).collect();
                fails.push(format!("{tool}/{}: {detail}", cmp.id));
            }
        }
        println!("{tool}\t{}\t{strict}\t{lenient}", cases.len());
    }
    let out = std::env::var("FAILS_OUT").unwrap_or_else(|_| "fails.txt".into());
    std::fs::write(out, fails.join("\n") + "\n")?;
    Ok(())
}

fn run_isolated(inv: Invocation) -> Outcome {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rt.block_on(run_case(&inv))
            }));
            let _ = tx.send(res.unwrap_or_else(|_| Outcome::unsupported("panic")));
        })
        .expect("spawn case thread");
    rx.recv_timeout(CASE_TIMEOUT).unwrap_or_else(|_| Outcome {
        timed_out: true,
        ..Outcome::unsupported("timeout")
    })
}

async fn run_case(inv: &Invocation) -> Outcome {
    let mut builder = Bash::builder()
        .cwd(harness::CASE_DIR)
        .limits(ExecutionLimits::cli())
        .username("root")
        .hostname("oracle")
        .git(GitConfig::new())
        .sqlite();
    for (k, v) in inv.full_env() {
        builder = builder.env(k, v);
    }
    if let Some(ft) = &inv.faketime {
        match faketime_epoch(ft) {
            Some(epoch) => builder = builder.fixed_epoch(epoch),
            None => return Outcome::unsupported(format!("unsupported faketime: {ft}")),
        }
    }
    let mut bash = builder.build();
    let fs = bash.fs();
    if let Err(e) = load_fixture(fs.as_ref(), &inv.files).await {
        return Outcome::unsupported(format!("fixture: {e}"));
    }
    let script = match &inv.script {
        Some(s) => s.clone(),
        None => inv.argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" "),
    };
    let opts = ExecOptions::new().stdin(inv.stdin.clone());
    let (stdout, stderr, exit) = match bash.exec_with_options(&script, opts).await {
        Ok(r) => (r.stdout.into_bytes(), r.stderr.into_bytes(), r.exit_code),
        // An API error (parse, limit) is the observable result of the case.
        Err(e) => {
            let code = if matches!(e, bashkit::Error::Parse { .. }) { 2 } else { 1 };
            (Vec::new(), format!("bashkit: {e}\n").into_bytes(), code)
        }
    };
    let files = match snapshot(fs.as_ref()).await {
        Ok(t) => t,
        Err(e) => return Outcome::unsupported(format!("snapshot: {e}")),
    };
    Outcome {
        stdout: Bytes(stdout),
        stderr: Bytes(stderr),
        exit: Some(exit),
        files,
        ..Outcome::default()
    }
}

async fn load_fixture(fs: &dyn FileSystem, tree: &MemTree) -> bashkit::Result<()> {
    let root = PathBuf::from(harness::CASE_DIR);
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(harness::FIXTURE_MTIME);
    fs.mkdir(&root, true).await?;
    // BTreeMap order: parents before children.
    for (rel, entry) in &tree.entries {
        let path = root.join(rel);
        match entry {
            Entry::Dir { .. } => fs.mkdir(&path, true).await?,
            Entry::File { data, .. } => {
                let bytes = data.as_ref().map(|d| d.0.clone()).unwrap_or_default();
                fs.write_file(&path, &bytes).await?;
            }
            Entry::Symlink { target } => fs.symlink(Path::new(target), &path).await?,
        }
    }
    // Modes and mtimes deepest first, so writing a child does not bump its parent.
    for (rel, entry) in tree.entries.iter().rev() {
        if let Entry::File { mode, .. } | Entry::Dir { mode } = entry {
            let path = root.join(rel);
            fs.chmod(&path, *mode).await?;
            fs.set_modified_time(&path, mtime).await?;
        }
    }
    fs.set_modified_time(&root, mtime).await
}

async fn snapshot(fs: &dyn FileSystem) -> bashkit::Result<MemTree> {
    let root = PathBuf::from(harness::CASE_DIR);
    let mut tree = MemTree::new();
    if !fs.exists(&root).await? {
        return Ok(tree);
    }
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for item in fs.read_dir(&dir).await? {
            let path = dir.join(&item.name);
            let rel = path
                .strip_prefix(&root)
                .expect("inside case dir")
                .to_string_lossy()
                .into_owned();
            let mode = item.metadata.mode & 0o7777;
            let entry = match item.metadata.file_type {
                FileType::Symlink => {
                    Entry::symlink(fs.read_link(&path).await?.to_string_lossy().into_owned())
                }
                FileType::Directory => {
                    stack.push(path);
                    Entry::dir(mode)
                }
                FileType::File => Entry::file(fs.read_file(&path).await?, mode),
                FileType::Fifo => Entry::file(Vec::new(), mode | 0o010000),
            };
            tree.entries.insert(rel, entry);
        }
    }
    Ok(tree)
}

/// `faketime` spec ("YYYY-MM-DD HH:MM:SS" UTC, or "@epoch") to epoch seconds.
fn faketime_epoch(spec: &str) -> Option<i64> {
    let spec = spec.trim();
    if let Some(rest) = spec.strip_prefix('@') {
        return rest.parse().ok();
    }
    let (date, time) = spec.split_once(' ').unwrap_or((spec, "00:00:00"));
    let d: Vec<i64> = date.split('-').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let t: Vec<i64> = time.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    if d.len() != 3 || t.is_empty() || t.len() > 3 {
        return None;
    }
    // Days since 1970-01-01, proleptic Gregorian (Howard Hinnant's algorithm).
    let y = if d[1] <= 2 { d[0] - 1 } else { d[0] };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * ((d[1] + 9) % 12) + 2) / 5 + d[2] - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    let secs = t[0] * 3600 + t.get(1).copied().unwrap_or(0) * 60 + t.get(2).copied().unwrap_or(0);
    Some(days * 86_400 + secs)
}

fn shell_quote(s: &str) -> String {
    let plain = !s.is_empty()
        && !s.starts_with('=')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_./=:,+@%-".contains(&b));
    if plain { s.to_string() } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

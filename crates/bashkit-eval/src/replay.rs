// Replay recorded agent sessions against the current bashkit and count gaps.
//
// Decisions (see knowledge/operations/eval.md#gap-telemetry):
// - The corpus is what agents actually typed: every bash tool call stored in
//   archived eval runs (`results/eval-*.json`, task files embedded) and mira
//   runs (`results/mira/*/cases/*/result.json`, task files from the dataset).
// - A session replays in order on one fresh `Bash` built exactly like the
//   eval agent's (username `eval`, hostname `bashkit-eval`, task files
//   mounted), so later calls see earlier calls' files.
// - A "gap" is a stderr line that says bashkit lacks something: a missing
//   command, an unknown option, an unsupported feature, or a parse error.
//   Each kind/key is counted at most once per call, both in the recorded
//   output and in today's replay, so the report shows what got fixed and
//   what still hurts, ranked by today's hits.
// - Replay needs no network and no model: it is cheap enough to rerun after
//   every fidelity PR. Output is JSON (site input) plus Markdown.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result};
use bashkit::{Bash, ExecutionLimits};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Per-call wall-clock cap; a replayed call that loops forever must not
/// stall the whole report.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Rows in the ranked gap table.
pub const TOP_N: usize = 20;

/// One recorded tool call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Call {
    pub commands: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub exit_code: i32,
}

/// One agent session: the task's starting files and the calls in order.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub source: String,
    pub task: String,
    pub files: BTreeMap<String, String>,
    pub calls: Vec<Call>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    MissingCommand,
    UnknownOption,
    Unsupported,
    ParseError,
}

impl GapKind {
    fn label(self) -> &'static str {
        match self {
            GapKind::MissingCommand => "missing command",
            GapKind::UnknownOption => "unknown option",
            GapKind::Unsupported => "unsupported",
            GapKind::ParseError => "parse error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Gap {
    pub kind: GapKind,
    pub key: String,
}

static MISSING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([^\s:]+): command not found").unwrap());
static OPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"([A-Za-z0-9_.\-]+): (?:unrecognized|invalid|unknown|illegal) (?:option|predicate|argument)\s*(?:--\s*)?[`'‘]?([^'’`\s]+)",
    )
    .unwrap()
});
static UNSUPPORTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"([A-Za-z0-9_.\-]+): .*(?:not supported|not implemented|not yet implemented|unsupported)",
    )
    .unwrap()
});
static PARSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:parse|syntax) error[:,]?\s*(.*)").unwrap());
/// Location and repeated error-kind prefixes, dropped so one message shape
/// is one key (`parse error: parse error at line 3, column 1: expected 'done'`
/// and `syntax error: expected 'done'` both become `expected 'done'`).
static PARSE_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:\s*(?:(?:parse|syntax) error|at line \d+(?:, column \d+)?)[:,]?)+\s*")
        .unwrap()
});
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

/// Gaps named by one stderr text, deduplicated.
pub fn classify(stderr: &str) -> BTreeSet<Gap> {
    let mut out = BTreeSet::new();
    for line in stderr.lines() {
        if let Some(c) = MISSING.captures(line) {
            out.insert(Gap {
                kind: GapKind::MissingCommand,
                key: c[1].to_string(),
            });
        } else if let Some(c) = OPTION.captures(line) {
            out.insert(Gap {
                kind: GapKind::UnknownOption,
                key: format!("{} {}", &c[1], &c[2]),
            });
        } else if let Some(c) = PARSE.captures(line) {
            // Positions differ per script; keep the message shape only.
            let msg = PARSE_NOISE.replace(c[1].trim(), "");
            let msg = DIGITS.replace_all(msg.trim(), "N");
            let msg: String = msg.chars().take(80).collect();
            out.insert(Gap {
                kind: GapKind::ParseError,
                key: if msg.is_empty() {
                    "(no detail)".into()
                } else {
                    msg
                },
            });
        } else if let Some(c) = UNSUPPORTED.captures(line) {
            out.insert(Gap {
                kind: GapKind::Unsupported,
                key: c[1].to_string(),
            });
        }
    }
    out
}

// --- corpus loading -------------------------------------------------------

#[derive(Deserialize)]
struct ArchivedRun {
    #[serde(default)]
    model: String,
    #[serde(default)]
    results: Vec<ArchivedResult>,
}

#[derive(Deserialize)]
struct ArchivedResult {
    task: ArchivedTask,
    trace: ArchivedTrace,
}

#[derive(Deserialize)]
struct ArchivedTask {
    id: String,
    #[serde(default)]
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct ArchivedTrace {
    #[serde(default)]
    tool_calls: Vec<Call>,
}

#[derive(Deserialize)]
struct MiraCase {
    sample: String,
    #[serde(default)]
    target: String,
    transcript: MiraTranscript,
}

#[derive(Deserialize)]
struct MiraTranscript {
    #[serde(default)]
    metadata: MiraMetadata,
}

#[derive(Deserialize, Default)]
struct MiraMetadata {
    #[serde(default)]
    bashkit: MiraBashkit,
}

#[derive(Deserialize, Default)]
struct MiraBashkit {
    #[serde(default)]
    tool_outputs: Vec<Call>,
}

#[derive(Deserialize)]
struct DatasetTask {
    id: String,
    #[serde(default)]
    files: BTreeMap<String, String>,
}

/// Load every recorded session under `results` (`eval-*.json` plus
/// `mira/*/cases/*/result.json`). `dataset` supplies mira tasks' files.
pub fn load_corpus(results: &Path, dataset: &Path) -> Result<Vec<Session>> {
    let tasks: HashMap<String, BTreeMap<String, String>> = std::fs::read_to_string(dataset)
        .with_context(|| format!("reading {}", dataset.display()))?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<DatasetTask>(l).map(|t| (t.id, t.files)))
        .collect::<std::result::Result<_, _>>()
        .context("parsing dataset")?;

    let mut sessions = Vec::new();
    let mut archived: Vec<PathBuf> = std::fs::read_dir(results)
        .with_context(|| format!("reading {}", results.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("eval-") && n.ends_with(".json"))
        })
        .collect();
    archived.sort();
    for path in archived {
        let run: ArchivedRun = serde_json::from_str(&std::fs::read_to_string(&path)?)
            .with_context(|| format!("parsing {}", path.display()))?;
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        for r in run.results {
            sessions.push(Session {
                source: format!("{name} ({})", run.model),
                task: r.task.id,
                files: r.task.files,
                calls: r.trace.tool_calls,
            });
        }
    }

    let mira = results.join("mira");
    if mira.is_dir() {
        let mut cases = Vec::new();
        for run in std::fs::read_dir(&mira)? {
            let cases_dir = run?.path().join("cases");
            if !cases_dir.is_dir() {
                continue;
            }
            for case in std::fs::read_dir(&cases_dir)? {
                let p = case?.path().join("result.json");
                if p.is_file() {
                    cases.push(p);
                }
            }
        }
        cases.sort();
        for p in cases {
            let case: MiraCase = serde_json::from_str(&std::fs::read_to_string(&p)?)
                .with_context(|| format!("parsing {}", p.display()))?;
            let run = p
                .ancestors()
                .nth(3)
                .and_then(|r| r.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // Samples no longer in the dataset can't be seeded faithfully.
            let Some(files) = tasks.get(&case.sample) else {
                continue;
            };
            sessions.push(Session {
                source: format!("mira {run} ({})", case.target),
                task: case.sample,
                files: files.clone(),
                calls: case.transcript.metadata.bashkit.tool_outputs,
            });
        }
    }
    sessions.retain(|s| !s.calls.is_empty());
    Ok(sessions)
}

// --- replay ---------------------------------------------------------------

/// Run a session's calls in order on a fresh `Bash`, as the eval agent did.
pub async fn replay(session: &Session) -> Vec<Call> {
    let mut builder = crate::agent::with_eval_runtimes(
        Bash::builder()
            .username("eval")
            .hostname("bashkit-eval")
            .limits(ExecutionLimits::default().timeout(CALL_TIMEOUT)),
    );
    for (path, content) in &session.files {
        builder = builder.mount_text(path, content);
    }
    let mut bash = builder.build();
    let mut out = Vec::with_capacity(session.calls.len());
    for call in &session.calls {
        let (stdout, stderr, exit_code) = match bash.exec(&call.commands).await {
            Ok(r) => (r.stdout.to_string(), r.stderr.to_string(), r.exit_code),
            Err(e) => (String::new(), e.to_string(), 1),
        };
        out.push(Call {
            commands: call.commands.clone(),
            stdout,
            stderr,
            exit_code,
        });
    }
    out
}

// --- report ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct GapRow {
    pub kind: GapKind,
    pub key: String,
    /// Calls hitting this gap in today's replay.
    pub now: usize,
    /// Calls hitting it in the recorded runs.
    pub recorded: usize,
    /// First command line that hit it (today, else recorded), trimmed.
    pub example: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Totals {
    /// Calls with at least one gap.
    pub gap_calls: usize,
    /// Calls that exited non-zero.
    pub failed_calls: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub generated_at: String,
    pub bashkit_version: String,
    pub sessions: usize,
    pub calls: usize,
    /// Calls whose stdout, stderr and exit code match the recording.
    pub calls_unchanged: usize,
    pub recorded: Totals,
    pub now: Totals,
    /// Gaps ranked by today's hits, then recorded hits (at most [`TOP_N`]).
    pub top: Vec<GapRow>,
    /// Gaps the recordings hit that today's replay no longer does.
    pub fixed: Vec<GapRow>,
}

#[derive(Default)]
struct Tally {
    now: usize,
    recorded: usize,
    example_now: Option<String>,
    example_recorded: Option<String>,
}

fn example_line(commands: &str, gap: &Gap) -> String {
    let needle = match gap.kind {
        GapKind::MissingCommand | GapKind::Unsupported => gap.key.as_str(),
        GapKind::UnknownOption => gap.key.split(' ').next().unwrap_or(""),
        GapKind::ParseError => "",
    };
    let line = commands
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && (needle.is_empty() || l.contains(needle)))
        .unwrap_or("");
    let mut s: String = line.chars().take(100).collect();
    if line.chars().count() > 100 {
        s.push('…');
    }
    s
}

/// Build the report from recorded sessions and their replays (same order).
pub fn build_report(sessions: &[Session], replays: &[Vec<Call>], generated_at: String) -> Report {
    let mut tally: BTreeMap<Gap, Tally> = BTreeMap::new();
    let mut recorded = Totals::default();
    let mut now = Totals::default();
    let mut calls = 0;
    let mut unchanged = 0;
    for (s, r) in sessions.iter().zip(replays) {
        for (old, new) in s.calls.iter().zip(r) {
            calls += 1;
            if old.stdout == new.stdout
                && old.stderr == new.stderr
                && old.exit_code == new.exit_code
            {
                unchanged += 1;
            }
            let og = classify(&old.stderr);
            let ng = classify(&new.stderr);
            recorded.gap_calls += usize::from(!og.is_empty());
            now.gap_calls += usize::from(!ng.is_empty());
            recorded.failed_calls += usize::from(old.exit_code != 0);
            now.failed_calls += usize::from(new.exit_code != 0);
            for g in og {
                let t = tally.entry(g.clone()).or_default();
                t.recorded += 1;
                t.example_recorded
                    .get_or_insert_with(|| example_line(&old.commands, &g));
            }
            for g in ng {
                let t = tally.entry(g.clone()).or_default();
                t.now += 1;
                t.example_now
                    .get_or_insert_with(|| example_line(&new.commands, &g));
            }
        }
    }
    let rows: Vec<GapRow> = tally
        .into_iter()
        .map(|(g, t)| GapRow {
            kind: g.kind,
            key: g.key,
            now: t.now,
            recorded: t.recorded,
            example: t.example_now.or(t.example_recorded).unwrap_or_default(),
        })
        .collect();
    let mut top: Vec<GapRow> = rows.iter().filter(|r| r.now > 0).cloned().collect();
    top.sort_by(|a, b| {
        (b.now, b.recorded)
            .cmp(&(a.now, a.recorded))
            .then_with(|| (a.kind, &a.key).cmp(&(b.kind, &b.key)))
    });
    top.truncate(TOP_N);
    let mut fixed: Vec<GapRow> = rows
        .into_iter()
        .filter(|r| r.now == 0 && r.recorded > 0)
        .collect();
    fixed.sort_by(|a, b| b.recorded.cmp(&a.recorded).then_with(|| a.key.cmp(&b.key)));
    Report {
        generated_at,
        bashkit_version: env!("CARGO_PKG_VERSION").to_string(),
        sessions: sessions.len(),
        calls,
        calls_unchanged: unchanged,
        recorded,
        now,
        top,
        fixed,
    }
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('`', "'")
}

/// Markdown rendering of a report.
pub fn render_markdown(r: &Report) -> String {
    let mut s = String::new();
    s.push_str("# Bashkit gap telemetry\n\n");
    s.push_str(&format!(
        "Replayed {} recorded agent sessions ({} tool calls) on bashkit {} at {}.\n\n",
        r.sessions, r.calls, r.bashkit_version, r.generated_at
    ));
    s.push_str("| | Recorded | Now |\n|---|---:|---:|\n");
    s.push_str(&format!(
        "| Calls hitting a gap | {} | {} |\n| Calls exiting non-zero | {} | {} |\n\n",
        r.recorded.gap_calls, r.now.gap_calls, r.recorded.failed_calls, r.now.failed_calls
    ));
    s.push_str(&format!(
        "{} of {} calls replay with identical stdout, stderr and exit code.\n\n",
        r.calls_unchanged, r.calls
    ));
    s.push_str(&format!("## Top {} gaps today\n\n", TOP_N));
    if r.top.is_empty() {
        s.push_str("None.\n\n");
    } else {
        s.push_str("| # | Kind | Gap | Calls now | Calls recorded | Example |\n|---:|---|---|---:|---:|---|\n");
        for (i, g) in r.top.iter().enumerate() {
            s.push_str(&format!(
                "| {} | {} | `{}` | {} | {} | `{}` |\n",
                i + 1,
                g.kind.label(),
                md_cell(&g.key),
                g.now,
                g.recorded,
                md_cell(&g.example)
            ));
        }
        s.push('\n');
    }
    s.push_str("## Fixed since recording\n\n");
    if r.fixed.is_empty() {
        s.push_str("None.\n");
    } else {
        s.push_str("| Kind | Gap | Calls recorded |\n|---|---|---:|\n");
        for g in &r.fixed {
            s.push_str(&format!(
                "| {} | `{}` | {} |\n",
                g.kind.label(),
                md_cell(&g.key),
                g.recorded
            ));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gaps(s: &str) -> Vec<(GapKind, String)> {
        classify(s).into_iter().map(|g| (g.kind, g.key)).collect()
    }

    #[test]
    fn classifies_missing_commands() {
        assert_eq!(
            gaps("bash: cmp: command not found\nbash: line 3: tsort: command not found. Try\n"),
            vec![
                (GapKind::MissingCommand, "cmp".into()),
                (GapKind::MissingCommand, "tsort".into())
            ]
        );
    }

    #[test]
    fn classifies_options() {
        assert_eq!(
            gaps(
                "ls: unrecognized option '--directory'\ngrep: invalid option -- 'P'\nfind: unknown predicate `-delete'\n"
            ),
            vec![
                (GapKind::UnknownOption, "find -delete".into()),
                (GapKind::UnknownOption, "grep P".into()),
                (GapKind::UnknownOption, "ls --directory".into()),
            ]
        );
    }

    #[test]
    fn classifies_unsupported_and_parse() {
        assert_eq!(
            gaps(
                "env: executing commands not supported in virtual mode\nbash: parse error at line 12, column 3: unexpected 'done'\n"
            ),
            vec![
                (GapKind::Unsupported, "env".into()),
                (GapKind::ParseError, "unexpected 'done'".into()),
            ]
        );
    }

    #[test]
    fn ordinary_errors_are_not_gaps() {
        assert!(
            gaps("cat: /nope: No such file or directory\ngrep: x: Is a directory\n").is_empty()
        );
        // One key per message shape, whatever the prefixes.
        assert_eq!(
            gaps(
                "bash: parse error: parse error at line 3, column 1: syntax error: expected 'done'\n"
            ),
            vec![(GapKind::ParseError, "expected 'done'".into())]
        );
        // Counted once per call.
        assert_eq!(
            gaps("bash: x: command not found\nbash: x: command not found\n").len(),
            1
        );
    }

    #[tokio::test]
    async fn replay_uses_task_files_and_carries_state() {
        let s = Session {
            source: "t".into(),
            task: "t".into(),
            files: BTreeMap::from([("/data/a.txt".into(), "hi\n".into())]),
            calls: vec![
                Call {
                    commands: "cp /data/a.txt /data/b.txt".into(),
                    ..Default::default()
                },
                Call {
                    commands: "cat /data/b.txt; nosuchcmd".into(),
                    ..Default::default()
                },
            ],
        };
        let out = replay(&s).await;
        assert_eq!(out[1].stdout, "hi\n");
        let r = build_report(&[s], &[out], "now".into());
        assert_eq!(r.calls, 2);
        assert_eq!(r.now.gap_calls, 1);
        assert_eq!(r.top[0].key, "nosuchcmd");
        assert_eq!(r.top[0].example, "cat /data/b.txt; nosuchcmd");
        assert!(render_markdown(&r).contains("`nosuchcmd`"));
    }

    #[test]
    fn report_lists_fixed_gaps() {
        let s = Session {
            calls: vec![Call {
                commands: "cmp a b".into(),
                stderr: "bash: cmp: command not found\n".into(),
                exit_code: 127,
                ..Default::default()
            }],
            ..Default::default()
        };
        let now = vec![vec![Call {
            commands: "cmp a b".into(),
            ..Default::default()
        }]];
        let r = build_report(&[s], &now, "now".into());
        assert!(r.top.is_empty());
        assert_eq!(r.fixed[0].key, "cmp");
        assert_eq!(r.recorded.failed_calls, 1);
        assert_eq!(r.now.failed_calls, 0);
    }
}

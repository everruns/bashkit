//! Checked-in competitor regressions: no network fetches, one data row per case.
//!
//! Important decision: CI executes reviewed fixture scripts inside Bashkit. The
//! separate host-oracle test only runs cases explicitly marked `real_bash`, in
//! a temporary directory, and never imports or downloads code (TM-INF-032).

use std::path::PathBuf;

use bashkit::Bash;
use serde::Deserialize;

#[cfg(feature = "http_client")]
use std::sync::Arc;

#[derive(Deserialize)]
struct Corpus {
    schema_version: u32,
    expected_case_count: usize,
    source: Source,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Source {
    repository: String,
    window_start: String,
    window_end: String,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    upstream_commit: String,
    upstream_date: String,
    classification: String,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    limitation_id: Option<String>,
    oracle: String,
    #[serde(default)]
    minimum_bash_major: Option<u32>,
    #[serde(default)]
    required_features: Vec<String>,
    #[serde(default)]
    transport: Option<String>,
    script: String,
    expected: Expected,
}

#[derive(Deserialize)]
struct Expected {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

// THREAT[TM-INF-032]: Only load reviewed, checked-in JSON; this harness never
// downloads upstream fixtures or generates executable source.
fn load_corpora() -> Vec<(String, Corpus)> {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/competitor-regressions");
    let mut paths = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("read corpus directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty(), "competitor corpus must not be empty");
    paths
        .into_iter()
        .map(|path| {
            let bytes =
                std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let corpus = serde_json::from_slice(&bytes)
                .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
            let name = path
                .file_name()
                .expect("corpus filename")
                .to_string_lossy()
                .into_owned();
            (name, corpus)
        })
        .collect()
}

#[allow(clippy::match_like_matches_macro)] // Each arm has an independent compile-time feature gate.
fn feature_available(name: &str) -> bool {
    match name {
        "http_client" => cfg!(feature = "http_client"),
        "jq" => cfg!(feature = "jq"),
        _ => false,
    }
}

fn host_program(name: &str) -> PathBuf {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(name))
                .find(|candidate| candidate.is_file())
        })
        .unwrap_or_else(|| PathBuf::from(name))
}

fn bash_major(program: &std::path::Path) -> u32 {
    let output = std::process::Command::new(program)
        .arg("--version")
        .output()
        .expect("query real bash version");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find_map(|word| word.split('.').next()?.parse().ok())
        .expect("parse real bash major version")
}

fn validate(case: &Case, source: &Source, limitations: &str) {
    assert!(
        case.id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "{}: id must be lowercase kebab-case",
        case.id
    );
    assert!(
        case.upstream_commit.len() == 40
            && case
                .upstream_commit
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "{}: upstream_commit must be a full Git object id",
        case.id
    );
    assert!(
        case.upstream_date.len() == 10
            && case.upstream_date.as_str() >= source.window_start.as_str()
            && case.upstream_date.as_str() <= source.window_end.as_str(),
        "{}: date {} must remain in {}..{}",
        case.id,
        case.upstream_date,
        source.window_start,
        source.window_end
    );
    assert!(
        matches!(
            case.classification.as_str(),
            "pass" | "bug" | "intentional_divergence"
        ),
        "{}: unknown classification {}",
        case.id,
        case.classification
    );
    match case.classification.as_str() {
        "bug" => {
            assert_eq!(case.resolution.as_deref(), Some("fixed"));
            assert!(case.limitation_id.is_none());
        }
        "intentional_divergence" => {
            let id = case
                .limitation_id
                .as_deref()
                .unwrap_or_else(|| panic!("{}: divergence must name a limitation ID", case.id));
            assert!(id.starts_with("L-"));
            assert!(
                limitations.contains(&format!("| {id} |")),
                "{}: limitation {id} is not canonical",
                case.id
            );
            assert!(case.resolution.is_none());
        }
        _ => {
            assert!(case.resolution.is_none());
            assert!(case.limitation_id.is_none());
        }
    }
    assert!(matches!(case.oracle.as_str(), "real_bash" | "locked"));
    if case.minimum_bash_major.is_some() {
        assert_eq!(case.oracle, "real_bash");
    }
    for feature in &case.required_features {
        assert!(matches!(feature.as_str(), "http_client" | "jq"));
    }
    assert!(
        matches!(case.transport.as_deref(), None | Some("echo_request_body")),
        "{}: unknown transport fixture",
        case.id
    );
    if case.transport.is_some() {
        assert!(
            case.required_features.iter().any(|f| f == "http_client"),
            "{}: transport fixture requires http_client",
            case.id
        );
    }
}

#[cfg(feature = "http_client")]
struct EchoRequestBody;

#[cfg(feature = "http_client")]
#[async_trait::async_trait]
impl bashkit::HttpTransport for EchoRequestBody {
    async fn execute(
        &self,
        request: bashkit::HttpTransportRequest,
    ) -> Result<bashkit::HttpResponse, bashkit::HttpTransportError> {
        Ok(bashkit::HttpResponse {
            status: 200,
            headers: vec![("content-type".into(), "text/plain".into())],
            body: request.body.unwrap_or_default(),
        })
    }
}

async fn run_bashkit(case: &Case) -> bashkit::ExecResult {
    let builder = Bash::builder();
    #[cfg(feature = "http_client")]
    let builder = if case.transport.as_deref() == Some("echo_request_body") {
        builder
            .network(bashkit::NetworkAllowlist::allow_all())
            .http_transport(Arc::new(EchoRequestBody))
    } else {
        builder
    };
    let mut bash = builder.build();
    bash.exec(&case.script).await.expect("execute fixture")
}

#[test]
fn competitor_corpus_has_complete_provenance_and_classification() {
    let limitations = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../knowledge/operations/limitations.md"),
    )
    .expect("read canonical limitations");
    let mut ids = std::collections::HashSet::new();
    for (name, corpus) in load_corpora() {
        assert_eq!(corpus.schema_version, 1, "{name}: unknown schema");
        assert!(
            corpus.source.repository.starts_with("https://"),
            "{name}: source repository must be HTTPS"
        );
        assert!(
            corpus.source.window_start.len() == 10
                && corpus.source.window_end.len() == 10
                && corpus.source.window_start <= corpus.source.window_end,
            "{name}: invalid source window"
        );
        assert_eq!(
            corpus.cases.len(),
            corpus.expected_case_count,
            "{name}: corpus selection is incomplete"
        );
        for case in &corpus.cases {
            assert!(
                ids.insert(case.id.clone()),
                "duplicate case id: {}",
                case.id
            );
            validate(case, &corpus.source, &limitations);
        }
    }
}

#[tokio::test]
async fn competitor_regressions_stay_green_without_network() {
    let mut failures = Vec::new();
    for (_, corpus) in load_corpora() {
        for case in corpus.cases {
            if !case.required_features.iter().all(|f| feature_available(f)) {
                continue;
            }
            let actual = run_bashkit(&case).await;
            if actual.stdout != case.expected.stdout
                || actual.stderr != case.expected.stderr
                || actual.exit_code != case.expected.exit_code
            {
                failures.push(format!(
                    "{}: expected ({:?}, {:?}, {}), got ({:?}, {:?}, {})",
                    case.id,
                    case.expected.stdout,
                    case.expected.stderr,
                    case.expected.exit_code,
                    actual.stdout,
                    actual.stderr,
                    actual.exit_code
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn competitor_real_bash_oracles_match_locked_expectations() {
    let bash = host_program("bash");
    let bash_major = bash_major(&bash);
    for (_, corpus) in load_corpora() {
        for case in corpus.cases {
            if case.oracle != "real_bash" {
                continue;
            }
            if let Some(minimum) = case.minimum_bash_major
                && bash_major < minimum
            {
                eprintln!(
                    "skip {}: bash {} is older than required major {}",
                    case.id, bash_major, minimum
                );
                continue;
            }
            let cwd = tempfile::tempdir().expect("oracle tempdir");
            let output = std::process::Command::new(&bash)
                .args(["--noprofile", "--norc", "-c", &case.script])
                .current_dir(cwd.path())
                .env_clear()
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                .env("LC_ALL", "C.UTF-8")
                .output()
                .expect("run real bash oracle");
            assert_eq!(
                output.stdout,
                case.expected.stdout.as_bytes(),
                "{} stdout",
                case.id
            );
            assert_eq!(
                output.stderr,
                case.expected.stderr.as_bytes(),
                "{} stderr",
                case.id
            );
            assert_eq!(
                output.status.code(),
                Some(case.expected.exit_code),
                "{} exit",
                case.id
            );
        }
    }
}

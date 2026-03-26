//! Integration tests for wedow/harness compatibility.
//!
//! Mounts harness bash scripts onto bashkit's VFS and verifies that the
//! core patterns — plugin discovery, hook pipelines, state machine, and
//! session management — execute correctly in the virtual interpreter.
//!
//! See: https://github.com/wedow/harness

use bashkit::Bash;
use std::path::Path;

// ---------------------------------------------------------------------------
// Harness core script (subset for testing)
// ---------------------------------------------------------------------------

const HARNESS_CORE: &str = r##"#!/usr/bin/env bash
# harness — minimal agent loop subset for bashkit integration testing.
set -euo pipefail

HARNESS_VERSION="0.1.0"
HARNESS_ROOT="${HARNESS_ROOT:-/opt/harness}"
readonly HARNESS_ROOT
HARNESS_HOME="${HARNESS_HOME:-/home/user/.harness}"
HARNESS_MODEL="${HARNESS_MODEL:-}"
HARNESS_PROVIDER="${HARNESS_PROVIDER:-}"
HARNESS_MAX_TURNS="${HARNESS_MAX_TURNS:-100}"
HARNESS_LOG="${HARNESS_LOG:-/dev/stderr}"

# State machine default transitions
declare -A DEFAULT_NEXT=(
  [start]=assemble
  [assemble]=send
  [send]=receive
  [receive]=done
  [tool_exec]=tool_done
  [tool_done]=assemble
  [error]=done
)

# Source discovery
_HARNESS_SOURCES=()

_discover_sources() {
  _HARNESS_SOURCES=()
  local d
  for d in "${HARNESS_ROOT}/plugins"/*/; do
    [[ -d "${d}" ]] || continue
    d="${d%/}"
    if [[ -d "${d}/providers" && -n "${HARNESS_PROVIDER}" && "$(basename "${d}")" != "${HARNESS_PROVIDER}" ]]; then
      continue
    fi
    _HARNESS_SOURCES+=("${d}")
  done

  local joined=""
  for d in "${_HARNESS_SOURCES[@]}"; do
    [[ -n "${joined}" ]] && joined+=":"
    joined+="${d}"
  done
  export HARNESS_SOURCES="${joined}"
}

# Tool discovery
_collect_tools_from() {
  local dir="$1"
  local -n tmap_ref="$2"
  [[ -d "${dir}" ]] || return 0
  local f
  for f in "${dir}"/*; do
    [[ -x "${f}" ]] || continue
    local name="$(basename "${f}")"
    tmap_ref["${name}"]="${f}"
  done
}

discover_tools() {
  _discover_sources
  declare -A tool_map
  local src
  for src in "${_HARNESS_SOURCES[@]}"; do
    _collect_tools_from "${src}/tools" tool_map
  done
  for name in "${!tool_map[@]}"; do
    echo "${name}"
  done | sort
}

# Hook discovery and execution
_collect_hooks_from() {
  local dir="$1"
  local -n map_ref="$2"
  [[ -d "${dir}" ]] || return 0
  local f
  for f in "${dir}"/*; do
    [[ -x "${f}" ]] || continue
    local base="$(basename "${f}")"
    map_ref["${base}"]="${f}"
  done
}

run_hooks() {
  local stage="$1"; shift

  declare -A hook_map
  local src
  for src in "${_HARNESS_SOURCES[@]}"; do
    _collect_hooks_from "${src}/hooks.d/${stage}" hook_map
  done

  # Sort hook names by numeric prefix
  local hooks=()
  for base in $(printf '%s\n' "${!hook_map[@]}" | sort); do
    hooks+=("${hook_map[$base]}")
  done

  local current=""
  [[ ! -t 0 ]] && current="$(cat)"

  for hook in "${hooks[@]:-}"; do
    [[ -z "${hook}" ]] && continue
    current="$(echo "${current}" | "${hook}" 2>/dev/null)"
    local rc=$?
    if (( rc != 0 )); then
      printf '%s' "${current}"
      return ${rc}
    fi
  done

  printf '%s' "${current}"
}

# Session management
session_new() {
  local id="test-session-$$"
  local dir="${HARNESS_HOME}/sessions/${id}"
  mkdir -p "${dir}/messages"
  cat > "${dir}/session.md" <<EOF
---
id: ${id}
model: ${HARNESS_MODEL}
provider: ${HARNESS_PROVIDER}
created: $(date -Iseconds)
cwd: ${PWD}
---
EOF
  echo "${dir}"
}

next_seq() {
  local dir="$1"
  local last
  last="$(ls -1 "${dir}/messages/" 2>/dev/null | sort -n | tail -1)"
  if [[ -z "${last}" ]]; then
    echo "0001"
  else
    printf '%04d' $(( 10#${last%%-*} + 1 ))
  fi
}

save_user_message() {
  local dir="$1" content="$2"
  local seq; seq="$(next_seq "${dir}")"
  cat > "${dir}/messages/${seq}-user.md" <<EOF
---
role: user
seq: ${seq}
timestamp: $(date -Iseconds)
---
${content}
EOF
}

_log() {
  echo "[harness] $*" >> "${HARNESS_LOG}"
}

_die() {
  echo "harness: $*" >&2
  exit 1
}

_require() {
  command -v "$1" &>/dev/null || _die "required command '$1' not found"
}

# Only run main when executed directly, not when sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  _require jq
  _discover_sources
  echo "harness v${HARNESS_VERSION} ready"
  echo "sources: ${#_HARNESS_SOURCES[@]}"
fi
"##;

// A simple tool plugin
const TOOL_ECHO: &str = r#"#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  --schema)
    cat <<'JSON'
{"name":"echo_tool","description":"Echo input back","input_schema":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}
JSON
    ;;
  --describe)
    echo "Echo input back"
    ;;
  --exec)
    input="$(cat)"
    text="$(echo "${input}" | jq -r '.text')"
    echo "${text}"
    ;;
  *)
    echo "usage: echo_tool --schema | --describe | --exec" >&2
    exit 1
    ;;
esac
"#;

// A simple hook
const HOOK_INIT: &str = r#"#!/usr/bin/env bash
set -euo pipefail
input="$(cat)"
echo "${input}"
"#;

const HOOK_ADD_FIELD: &str = r#"#!/usr/bin/env bash
set -euo pipefail
input="$(cat)"
echo "${input}" | jq --arg v ready '. + {status: $v}'
"#;

async fn setup_harness() -> Bash {
    let bash = Bash::builder()
        .env("HOME", "/home/user")
        .env("HARNESS_ROOT", "/opt/harness")
        .env("HARNESS_HOME", "/home/user/.harness")
        .env("HARNESS_LOG", "/dev/null")
        .cwd("/home/user")
        .build();
    let fs = bash.fs();

    // Mount harness core
    fs.mkdir(Path::new("/opt/harness/bin"), true).await.unwrap();
    fs.write_file(
        Path::new("/opt/harness/bin/harness"),
        HARNESS_CORE.as_bytes(),
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/opt/harness/bin/harness"), 0o755)
        .await
        .unwrap();

    // Mount plugin structure
    let dirs = [
        "/opt/harness/plugins/core/tools",
        "/opt/harness/plugins/core/hooks.d/start",
        "/opt/harness/plugins/core/hooks.d/assemble",
        "/opt/harness/plugins/core/commands",
        "/home/user/.harness/sessions",
    ];
    for dir in &dirs {
        fs.mkdir(Path::new(dir), true).await.unwrap();
    }

    // Mount tool
    fs.write_file(
        Path::new("/opt/harness/plugins/core/tools/echo_tool"),
        TOOL_ECHO.as_bytes(),
    )
    .await
    .unwrap();
    fs.chmod(
        Path::new("/opt/harness/plugins/core/tools/echo_tool"),
        0o755,
    )
    .await
    .unwrap();

    // Mount hooks
    fs.write_file(
        Path::new("/opt/harness/plugins/core/hooks.d/start/10-init"),
        HOOK_INIT.as_bytes(),
    )
    .await
    .unwrap();
    fs.chmod(
        Path::new("/opt/harness/plugins/core/hooks.d/start/10-init"),
        0o755,
    )
    .await
    .unwrap();

    fs.write_file(
        Path::new("/opt/harness/plugins/core/hooks.d/assemble/20-add"),
        HOOK_ADD_FIELD.as_bytes(),
    )
    .await
    .unwrap();
    fs.chmod(
        Path::new("/opt/harness/plugins/core/hooks.d/assemble/20-add"),
        0o755,
    )
    .await
    .unwrap();

    bash
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Harness core script executes and reports version
#[tokio::test]
async fn harness_core_runs() {
    let mut bash = setup_harness().await;
    let result = bash.exec("/opt/harness/bin/harness").await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "exit_code={}, stderr={}, stdout={}",
        result.exit_code, result.stderr, result.stdout
    );
    assert!(
        result.stdout.contains("harness v0.1.0 ready"),
        "stdout: {}",
        result.stdout
    );
}

/// Source guard: when sourced, main block does not run
#[tokio::test]
async fn harness_source_guard() {
    let mut bash = setup_harness().await;
    let result = bash
        .exec(
            r#"
source /opt/harness/bin/harness
echo "sourced ok"
echo "version: ${HARNESS_VERSION}"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert!(
        result.stdout.contains("sourced ok"),
        "stdout: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("version: 0.1.0"),
        "stdout: {}",
        result.stdout
    );
    // Should NOT contain the direct-execution output
    assert!(
        !result.stdout.contains("harness v0.1.0 ready"),
        "main block ran when sourced: {}",
        result.stdout
    );
}

/// Source discovery finds plugins
#[tokio::test]
async fn harness_source_discovery() {
    let mut bash = setup_harness().await;
    let result = bash
        .exec(
            r#"
source /opt/harness/bin/harness
_discover_sources
echo "${#_HARNESS_SOURCES[@]}"
echo "${_HARNESS_SOURCES[0]}"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert!(
        lines[0].parse::<usize>().unwrap() >= 1,
        "should discover at least 1 source"
    );
    assert!(
        lines[1].starts_with("/opt/harness/plugins/"),
        "first source should be a plugin dir"
    );
}

/// Tool discovery finds mounted tools
#[tokio::test]
async fn harness_tool_discovery() {
    let mut bash = setup_harness().await;
    let result = bash
        .exec(
            r#"
source /opt/harness/bin/harness
discovered="$(discover_tools)"
echo "${discovered}"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert!(
        result.stdout.contains("echo_tool"),
        "should discover echo_tool: {}",
        result.stdout
    );
}

/// Tool execution via --schema, --describe, --exec protocol
#[tokio::test]
async fn harness_tool_protocol() {
    let mut bash = setup_harness().await;
    // Test --describe
    let result = bash
        .exec(
            r#"
/opt/harness/plugins/core/tools/echo_tool --describe
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "Echo input back");

    // Test --schema returns valid JSON
    let result = bash
        .exec(
            r#"
schema="$(/opt/harness/plugins/core/tools/echo_tool --schema)"
echo "${schema}" | jq -r '.name'
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "echo_tool");

    // Test --exec with pipe stdin (harness tool_exec pattern)
    let result = bash
        .exec(
            r#"
echo '{"text":"hello world"}' | /opt/harness/plugins/core/tools/echo_tool --exec
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "hello world");
}

/// Tool schema via --schema flag
#[tokio::test]
async fn harness_tool_schema() {
    let mut bash = setup_harness().await;
    let result = bash
        .exec(
            r#"
schema="$(/opt/harness/plugins/core/tools/echo_tool --schema)"
echo "${schema}" | jq -r '.name'
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "echo_tool");
}

/// Hook pipeline: data flows through hooks via stdin/stdout chain
#[tokio::test]
async fn harness_hook_pipeline() {
    let mut bash = setup_harness().await;
    // Test the full run_hooks pipeline — data flows through each hook
    let result = bash
        .exec(
            r#"
source /opt/harness/bin/harness
_discover_sources
result="$(echo '{"stage":"start"}' | run_hooks start)"
echo "${result}" | jq -r '.stage'
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "start", "stdout: {}", result.stdout);
}

/// Session creation and message saving
#[tokio::test]
async fn harness_session_management() {
    let mut bash = setup_harness().await;
    let result = bash
        .exec(
            r#"
source /opt/harness/bin/harness
session_dir="$(session_new)"
echo "dir: ${session_dir}"
save_user_message "${session_dir}" "Hello, agent!"
# Verify files exist
ls "${session_dir}/messages/" | head -1
cat "${session_dir}/messages/0001-user.md" | grep "Hello, agent!"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert!(
        result.stdout.contains("dir: /home/user/.harness/sessions/"),
        "stdout: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("Hello, agent!"),
        "message should be saved: {}",
        result.stdout
    );
}

/// Sequence number generation
#[tokio::test]
async fn harness_sequence_numbers() {
    let mut bash = setup_harness().await;
    let result = bash
        .exec(
            r#"
source /opt/harness/bin/harness
session_dir="$(session_new)"
save_user_message "${session_dir}" "first"
ls -1 "${session_dir}/messages/" | head -1
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert!(
        result.stdout.contains("0001-user"),
        "first message should have 0001 prefix: {}",
        result.stdout
    );
}

/// State machine transitions
#[tokio::test]
async fn harness_state_machine() {
    let mut bash = setup_harness().await;
    let result = bash
        .exec(
            r#"
source /opt/harness/bin/harness
echo "${DEFAULT_NEXT[start]}"
echo "${DEFAULT_NEXT[assemble]}"
echo "${DEFAULT_NEXT[receive]}"
echo "${DEFAULT_NEXT[tool_exec]}"
echo "${DEFAULT_NEXT[error]}"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert_eq!(lines, vec!["assemble", "send", "done", "tool_done", "done"]);
}

/// exec builtin dispatches to script
#[tokio::test]
async fn harness_exec_dispatch() {
    let mut bash = setup_harness().await;
    let fs = bash.fs();

    fs.write_file(
        Path::new("/opt/harness/plugins/core/commands/version"),
        b"#!/bin/bash\necho \"v0.1.0\"",
    )
    .await
    .unwrap();
    fs.chmod(
        Path::new("/opt/harness/plugins/core/commands/version"),
        0o755,
    )
    .await
    .unwrap();

    let result = bash
        .exec("exec /opt/harness/plugins/core/commands/version")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "v0.1.0");
}

/// Dependency checking via command -v
#[tokio::test]
async fn harness_require_check() {
    let mut bash = setup_harness().await;
    let result = bash
        .exec(
            r#"
source /opt/harness/bin/harness
_require jq && echo "jq ok"
_require curl && echo "curl ok"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert!(result.stdout.contains("jq ok"));
    assert!(result.stdout.contains("curl ok"));
}

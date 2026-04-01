#!/usr/bin/env bash
# Run wedow/harness (https://github.com/wedow/harness) through bashkit CLI.
#
# Decision: mount harness repo read-only into bashkit VFS, source harness core,
# and drive the agent loop directly — avoids exec of scripts from VFS which
# bashkit does not support.
#
# Requires:
#   - bashkit-cli built with --features realfs
#   - OPENAI_API_KEY (or OPENAI_API_URL + OPENAI_API_KEY for compatible APIs)
#   - harness repo cloned to $HARNESS_CLONE_DIR (default: /tmp/harness)
#
# Usage:
#   export OPENAI_API_KEY="sk-..."
#   ./examples/harness_openai_joke.sh
#
#   # Or with OpenRouter:
#   export OPENAI_API_KEY="sk-or-v1-..."
#   export OPENAI_API_URL="https://openrouter.ai/api/v1/chat/completions"
#   export HARNESS_MODEL="openai/gpt-4o-mini"
#   ./examples/harness_openai_joke.sh
set -euo pipefail

HARNESS_CLONE_DIR="${HARNESS_CLONE_DIR:-/tmp/harness}"
HARNESS_MODEL="${HARNESS_MODEL:-gpt-4o}"

if [[ ! -d "${HARNESS_CLONE_DIR}" ]]; then
  echo "Cloning wedow/harness to ${HARNESS_CLONE_DIR}..."
  git clone https://github.com/wedow/harness.git "${HARNESS_CLONE_DIR}"
fi

if [[ -z "${OPENAI_API_KEY:-}" ]]; then
  echo "error: OPENAI_API_KEY is required" >&2
  exit 1
fi

# Write the inner script to a temp dir (bashkit needs a directory mount, not a file)
INNER_DIR="$(mktemp -d)"
trap 'rm -rf "${INNER_DIR}"' EXIT

cat > "${INNER_DIR}/run.sh" << 'INNER_EOF'
export HOME="${HOME:-/root}"
export HARNESS_THINKING="${HARNESS_THINKING:-}"
export HARNESS_STREAM="${HARNESS_STREAM:-}"
export HARNESS_STREAM_FD="${HARNESS_STREAM_FD:-}"
export HARNESS_TOOL_FIFO="${HARNESS_TOOL_FIFO:-}"

export HARNESS_ROOT=/tmp/harness
export HARNESS_HOME=/tmp/harness_home
export HARNESS_SESSIONS=/tmp/harness_home/sessions
export HARNESS_LOG=/tmp/harness_home/harness.log
export HARNESS_MAX_TURNS=3

mkdir -p "${HARNESS_HOME}" "${HARNESS_SESSIONS}"

source /tmp/harness/bin/harness

_refresh_sources
_resolve="$(jq -n --arg p "${HARNESS_PROVIDER}" --arg m "${HARNESS_MODEL}" \
  '{provider: $p, model: $m}' | call resolve)"
HARNESS_PROVIDER="$(echo "${_resolve}" | jq -r '.provider // empty')"
HARNESS_MODEL="$(echo "${_resolve}" | jq -r '.model // empty')"
export HARNESS_PROVIDER HARNESS_MODEL

echo "provider: ${HARNESS_PROVIDER}" >&2
echo "model: ${HARNESS_MODEL}" >&2

session_id="$(date +%Y%m%d-%H%M%S)-$$"
session_dir="${HARNESS_SESSIONS}/${session_id}"
mkdir -p "${session_dir}/messages"

printf 'id=%s\nmodel=%s\nprovider=%s\ncreated=%s\ncwd=%s\n' \
  "${session_id}" "${HARNESS_MODEL}" "${HARNESS_PROVIDER}" \
  "$(date -Iseconds)" "${PWD}" > "${session_dir}/session.conf"

echo "session: ${session_id}" >&2

printf -- '---\nrole: user\nseq: 0001\ntimestamp: %s\n---\nTell me a short joke. Just reply with the joke, nothing else.\n' \
  "$(date -Iseconds)" > "${session_dir}/messages/0001-user.md"

agent_loop "${session_dir}"
INNER_EOF

echo "Running harness via bashkit..."
echo "---"

cargo run -p bashkit-cli --features realfs -- \
  --mount-ro "${HARNESS_CLONE_DIR}:/tmp/harness" \
  --mount-ro "${INNER_DIR}:/tmp/inner" \
  -c "
export OPENAI_API_KEY='${OPENAI_API_KEY}'
export OPENAI_API_URL='${OPENAI_API_URL:-https://api.openai.com/v1/chat/completions}'
export HARNESS_PROVIDER=openai
export HARNESS_MODEL='${HARNESS_MODEL}'
source /tmp/inner/run.sh
"

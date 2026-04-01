#!/usr/bin/env bash
# harness_openai_joke.sh — Run wedow/harness agent loop via bashkit.
#
# Runs the full harness framework (github.com/wedow/harness) inside bashkit's
# virtual bash interpreter. Harness is a minimal agent loop in bash where
# everything else is plugins — this demonstrates bashkit executing the entire
# state machine: source discovery, provider resolution, session init, message
# assembly, OpenAI API call, response parsing, and output.
#
# Prerequisites:
#   git clone https://github.com/wedow/harness /tmp/harness
#
# Bashkit's curl doesn't support `-d @-` (stdin piping), so the openai
# provider needs a one-line patch: `sed -i 's/-d @-/-d "$request"/'`.
#
# Usage:
#   # Patch harness for bashkit curl compatibility
#   cp -r /tmp/harness /tmp/harness-patched
#   sed -i 's/-d @-/-d "$request"/' /tmp/harness-patched/plugins/openai/providers/openai
#
#   # Run via bashkit with host mount
#   bashkit --mount-ro /tmp/harness-patched:/harness -c "
#     export OPENAI_API_KEY='sk-...'
#     export OPENAI_API_URL='https://openrouter.ai/api/v1/chat/completions'
#     export HARNESS_MODEL='openai/gpt-4o'
#     source /harness-run.sh
#   "
#
# Or with direct OpenAI:
#   bashkit --mount-ro /tmp/harness-patched:/harness -c "
#     export OPENAI_API_KEY='sk-...'
#     export HARNESS_MODEL='gpt-4o'
#     source /harness-run.sh
#   "
set -euo pipefail

# --- Harness environment setup ---
export HOME="${HOME:-/root}"
export HARNESS_ROOT="${HARNESS_ROOT:-/harness}"
export HARNESS_HOME="${HARNESS_HOME:-${HOME}/.harness}"
export HARNESS_PROVIDER="${HARNESS_PROVIDER:-openai}"
export HARNESS_MODEL="${HARNESS_MODEL:-gpt-4o}"
export OPENAI_API_URL="${OPENAI_API_URL:-https://api.openai.com/v1/chat/completions}"
export HARNESS_MAX_TURNS="${HARNESS_MAX_TURNS:-1}"
export HARNESS_LOG="${HARNESS_LOG:-/dev/null}"
export HARNESS_STREAM="${HARNESS_STREAM:-0}"
export HARNESS_THINKING="${HARNESS_THINKING:-off}"

if [[ -z "${OPENAI_API_KEY:-}" ]]; then
  echo "error: OPENAI_API_KEY is required" >&2
  exit 1
fi

mkdir -p "${HARNESS_HOME}/sessions"

# Run the full harness agent loop
exec "${HARNESS_ROOT}/plugins/core/commands/agent" "Tell me a joke"

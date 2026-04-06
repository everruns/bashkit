#!/usr/bin/env bash
# Run the wedow/harness agent framework via bashkit to generate a joke using OpenAI.
#
# Prerequisites:
#   - cargo build -p bashkit-cli --features realfs
#   - OPENAI_API_KEY set in environment
#
# Usage:
#   bash examples/harness-openai-joke.sh
#   OPENAI_API_KEY=sk-... bash examples/harness-openai-joke.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BASHKIT="${BASHKIT:-$PROJECT_ROOT/target/debug/bashkit}"

# Build if binary doesn't exist
if [[ ! -x "$BASHKIT" ]]; then
  echo "Building bashkit CLI with realfs support..."
  cargo build -p bashkit-cli --features realfs --quiet
fi

HARNESS_DIR="${HARNESS_DIR:-/tmp/harness}"
WORK_DIR="${WORK_DIR:-/tmp/harness-work}"

if [[ ! -d "${HARNESS_DIR}" ]]; then
  echo "Cloning harness..."
  git clone https://github.com/wedow/harness "${HARNESS_DIR}"
fi

mkdir -p "${WORK_DIR}/.harness/sessions"

: "${OPENAI_API_KEY:?OPENAI_API_KEY must be set}"

exec "$BASHKIT" \
  --mount-ro "${HARNESS_DIR}:/harness" \
  --mount-rw "${WORK_DIR}:/work" \
  --timeout 120 \
  -c '
export PATH="/harness/bin:${PATH}"
export HOME=/work
export HARNESS_ROOT=/harness
export HARNESS_PROVIDER=openai
export HARNESS_MODEL=gpt-4o
export HARNESS_MAX_TURNS=3
export OPENAI_API_KEY="'"${OPENAI_API_KEY}"'"
mkdir -p /work/.harness/sessions
hs "tell me a short joke"

# Other commands that work inside bashkit:
#   hs help            — show providers, tools, plugin dirs
#   hs session list    — list past sessions
'

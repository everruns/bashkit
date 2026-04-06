#!/usr/bin/env bash
# Run the wedow/harness agent framework via bashkit to generate a joke using OpenAI.
#
# Prerequisites:
#   - bashkit installed: cargo install --path ./crates/bashkit-cli --features realfs
#   - harness cloned:    git clone https://github.com/wedow/harness /tmp/harness
#   - OPENAI_API_KEY set in environment
#
# Usage:
#   ./examples/harness-openai-joke.sh
#   OPENAI_API_KEY=sk-... ./examples/harness-openai-joke.sh
set -euo pipefail

HARNESS_DIR="${HARNESS_DIR:-/tmp/harness}"
WORK_DIR="${WORK_DIR:-/tmp/harness-work}"

if [[ ! -d "${HARNESS_DIR}" ]]; then
  echo "Cloning harness..."
  git clone https://github.com/wedow/harness "${HARNESS_DIR}"
fi

mkdir -p "${WORK_DIR}/.harness/sessions"

: "${OPENAI_API_KEY:?OPENAI_API_KEY must be set}"

exec bashkit \
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
'

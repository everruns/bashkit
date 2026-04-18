#!/usr/bin/env bash
# Run the wedow/harness agent framework via bashkit to generate a joke using OpenAI.
#
# Decision: install a local non-streaming OpenAI provider override in HARNESS_HOME.
# Harness auto-enables streaming when the provider advertises `--stream`, but the
# bashkit realfs mount used by this example does not support mkfifo for the FIFO
# dispatcher path yet.
#
# Decision: treat `error:` output as failure. The harness loop can print an error
# message while the surrounding bashkit invocation still exits 0, which hides
# breakage in CI unless this script checks the output explicitly.
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
HARNESS_HOME="${HARNESS_HOME:-${WORK_DIR}/.harness}"

if [[ ! -d "${HARNESS_DIR}" ]]; then
  echo "Cloning harness..."
  git clone https://github.com/wedow/harness "${HARNESS_DIR}"
fi

mkdir -p "${HARNESS_HOME}/sessions" "${HARNESS_HOME}/providers"

cat > "${HARNESS_HOME}/providers/openai" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec /harness/plugins/openai/providers/openai "$@"
EOF
chmod +x "${HARNESS_HOME}/providers/openai"

: "${OPENAI_API_KEY:?OPENAI_API_KEY must be set}"

output_file="$(mktemp)"
trap 'rm -f "${output_file}"' EXIT

"$BASHKIT" \
  --mount-ro "${HARNESS_DIR}:/harness" \
  --mount-rw "${WORK_DIR}:/work" \
  --timeout 120 \
  -c '
export PATH="/harness/bin:${PATH}"
export HOME=/work
export HARNESS_HOME=/work/.harness
export HARNESS_ROOT=/harness
export HARNESS_PROVIDER=openai
export HARNESS_MODEL=gpt-4o
export HARNESS_MAX_TURNS=3
export OPENAI_API_KEY="'"${OPENAI_API_KEY}"'"
hs "tell me a short joke"

# Other commands that work inside bashkit:
#   hs help            — show providers, tools, plugin dirs
#   hs session list    — list past sessions
' | tee "${output_file}"

status=${PIPESTATUS[0]}
if (( status != 0 )); then
  exit "${status}"
fi

if grep -q '^error:' "${output_file}"; then
  exit 1
fi

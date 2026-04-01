#!/usr/bin/env bash
# harness_openai_joke.sh — Harness-style OpenAI agent call via bashkit.
#
# Demonstrates the wedow/harness agent pattern (provider resolution, request
# assembly, API call, response parsing) running inside bashkit's virtual bash.
#
# Harness uses an OpenAI-compatible provider that builds a chat completions
# request with jq and sends it via curl. This script captures that same
# pattern in a single file bashkit can execute end-to-end.
#
# Usage:
#   bashkit -c "
#     export OPENAI_API_KEY='sk-...'
#     export HARNESS_MODEL='gpt-4o'
#     $(cat examples/harness_openai_joke.sh)
#   "
#
# Or with OpenRouter:
#   bashkit -c "
#     export OPENAI_API_KEY='sk-or-...'
#     export OPENAI_API_URL='https://openrouter.ai/api/v1/chat/completions'
#     export HARNESS_MODEL='openai/gpt-4o'
#     $(cat examples/harness_openai_joke.sh)
#   "
set -euo pipefail

# --- Harness provider config ---
HARNESS_PROVIDER="${HARNESS_PROVIDER:-openai}"
HARNESS_MODEL="${HARNESS_MODEL:-gpt-4o}"
OPENAI_API_URL="${OPENAI_API_URL:-https://api.openai.com/v1/chat/completions}"
OPENAI_MAX_TOKENS="${OPENAI_MAX_TOKENS:-1024}"

if [[ -z "${OPENAI_API_KEY:-}" ]]; then
  echo "error: OPENAI_API_KEY is required" >&2
  exit 1
fi

# --- Harness assemble stage: build messages + request ---
system_prompt="You are a helpful assistant. When asked, tell a creative, original joke. Keep it short and punchy."
user_message="Tell me a joke"

request=$(jq -n \
  --arg model "$HARNESS_MODEL" \
  --argjson max_tokens "$OPENAI_MAX_TOKENS" \
  --arg system "$system_prompt" \
  --arg user "$user_message" \
  '{
    model: $model,
    max_tokens: $max_tokens,
    messages: [
      {role: "system", content: $system},
      {role: "user", content: $user}
    ]
  }')

# --- Harness send stage: call OpenAI-compatible API ---
response=$(curl -sS "$OPENAI_API_URL" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "content-type: application/json" \
  -d "$request" \
  --max-time 60)

# --- Harness receive stage: parse response ---
error_msg=$(echo "$response" | jq -r '.error.message // empty')
if [[ -n "$error_msg" ]]; then
  echo "openai API error: $error_msg" >&2
  exit 1
fi

content=$(echo "$response" | jq -r '.choices[0].message.content // empty')
model_used=$(echo "$response" | jq -r '.model // empty')
tokens=$(echo "$response" | jq -r '.usage.total_tokens // "?"')

echo "provider: $HARNESS_PROVIDER"
echo "model: $model_used"
echo "tokens: $tokens"
echo "---"
echo "$content"

#!/usr/bin/env bash
# Shared helpers for bashkit development scripts.

set -euo pipefail

GIT_AGENT_IDENTITY_PATTERN="(claude|cursor|copilot|github-actions|bot|ai-agent|openai|anthropic|gpt)"

# Returns 0 if the value matches a known agent/bot pattern.
git_identity_looks_agent_like() {
  local value="${1:-}"
  [ -z "$value" ] && return 1
  printf '%s\n' "$value" | grep -iEq "$GIT_AGENT_IDENTITY_PATTERN"
}

# Resolves a human git identity from git config or env vars.
# Sets RESOLVED_GIT_AUTHOR_NAME, RESOLVED_GIT_AUTHOR_EMAIL, RESOLVED_GIT_AUTHOR_SOURCE.
resolve_commit_git_identity() {
  local current_name current_email
  current_name="$(git config user.name 2>/dev/null || true)"
  current_email="$(git config user.email 2>/dev/null || true)"

  RESOLVED_GIT_AUTHOR_NAME=""
  RESOLVED_GIT_AUTHOR_EMAIL=""
  RESOLVED_GIT_AUTHOR_SOURCE=""

  if [ -n "$current_name" ] && [ -n "$current_email" ] && \
    ! git_identity_looks_agent_like "$current_name" && \
    ! git_identity_looks_agent_like "$current_email"; then
    RESOLVED_GIT_AUTHOR_NAME="$current_name"
    RESOLVED_GIT_AUTHOR_EMAIL="$current_email"
    RESOLVED_GIT_AUTHOR_SOURCE="git"
  else
    if [ -z "${GIT_USER_NAME:-}" ] || [ -z "${GIT_USER_EMAIL:-}" ]; then
      echo "git commit identity is missing or agent-like; set GIT_USER_NAME and GIT_USER_EMAIL to a real user before committing" >&2
      return 1
    fi

    RESOLVED_GIT_AUTHOR_NAME="$GIT_USER_NAME"
    RESOLVED_GIT_AUTHOR_EMAIL="$GIT_USER_EMAIL"
    RESOLVED_GIT_AUTHOR_SOURCE="env"
  fi

  if git_identity_looks_agent_like "$RESOLVED_GIT_AUTHOR_NAME" || \
    git_identity_looks_agent_like "$RESOLVED_GIT_AUTHOR_EMAIL"; then
    echo "resolved git commit identity looks agent-like: '$RESOLVED_GIT_AUTHOR_NAME <$RESOLVED_GIT_AUTHOR_EMAIL>'" >&2
    return 1
  fi
}

# Configures git user.name/email if current identity is missing or agent-like.
configure_commit_git_identity_if_needed() {
  resolve_commit_git_identity || return 1
  git config user.name "$RESOLVED_GIT_AUTHOR_NAME"
  git config user.email "$RESOLVED_GIT_AUTHOR_EMAIL"
}

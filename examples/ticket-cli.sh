#!/usr/bin/env bash
# Run the wedow/ticket issue tracker inside bashkit with plugin support.
#
# Demonstrates: VFS mounts, PATH-based plugin discovery, complex bash scripts
# with awk, sed, and YAML frontmatter parsing — all interpreted by bashkit.
#
# Prerequisites:
#   - cargo build -p bashkit-cli --features realfs
#
# Usage:
#   bash examples/ticket-cli.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BASHKIT="${BASHKIT:-$PROJECT_ROOT/target/debug/bashkit}"

# Build if binary doesn't exist
if [[ ! -x "$BASHKIT" ]]; then
  echo "Building bashkit CLI with realfs support..."
  cargo build -p bashkit-cli --features realfs --quiet
fi

TICKET_DIR="${TICKET_DIR:-/tmp/bashkit-ticket}"
WORK_DIR="${WORK_DIR:-/tmp/bashkit-ticket-work}"

if [[ ! -d "$TICKET_DIR" ]]; then
  echo "Cloning wedow/ticket..."
  git clone --depth 1 https://github.com/wedow/ticket "$TICKET_DIR"
fi

mkdir -p "$WORK_DIR"

exec "$BASHKIT" \
  --mount-ro "$TICKET_DIR:/ticket" \
  --mount-rw "$WORK_DIR:/work" \
  --timeout 60 \
  -c '
export PATH="/ticket/plugins:$PATH"
export TK_SCRIPT="/ticket/ticket"
cd /work

# --- Create tickets ---
echo "=== Create tickets ==="
id1=$(/ticket/ticket create "Fix auth bypass" -t bug -p 1 -a alice --tags security,backend -d "JWT validation skipped on /admin routes")
id2=$(/ticket/ticket create "Add rate limiting" -t feature -p 2 -a bob --tags api,backend -d "Throttle API to 100 req/min per key")
id3=$(/ticket/ticket create "Update API docs" -t chore -p 3 -a alice --tags docs -d "Document new rate-limit headers")
echo "Created: $id1, $id2, $id3"

# --- Plugin: list tickets (ticket-ls) ---
echo ""
echo "=== Plugin: ticket-ls ==="
/ticket/ticket ls

# --- Manage workflow ---
echo ""
echo "=== Workflow: start, deps, notes ==="
/ticket/ticket start "$id1"
/ticket/ticket dep "$id3" "$id2"
/ticket/ticket add-note "$id1" "Root cause: missing middleware on /admin/* routes"

# --- Show a ticket with full detail ---
echo ""
echo "=== Show ticket ==="
/ticket/ticket show "$id1"

# --- Check ready vs blocked ---
echo ""
echo "=== Ready tickets ==="
/ticket/ticket ready
echo ""
echo "=== Blocked tickets ==="
/ticket/ticket blocked

# --- Dependency tree ---
echo ""
echo "=== Dependency tree ==="
/ticket/ticket dep tree "$id3"

# --- Close a ticket, check unblocking ---
echo ""
echo "=== Close rate-limiting ticket ==="
/ticket/ticket close "$id2"
echo ""
echo "=== Blocked after closing dep ==="
/ticket/ticket blocked

# --- Plugin: query as JSON ---
echo ""
echo "=== Plugin: ticket-query (JSON) ==="
/ticket/ticket query

# --- Plugin: list with filters ---
echo ""
echo "=== Plugin: ticket-ls --status=open ==="
/ticket/ticket ls --status=open
echo ""
echo "=== Plugin: ticket-ls -a alice ==="
/ticket/ticket ls -a alice

# Other commands that work inside bashkit:
#   /ticket/ticket show <id>     — display ticket with relationships
#   /ticket/ticket link <a> <b>  — link tickets together
#   /ticket/ticket dep cycle     — find dependency cycles
#   /ticket/ticket reopen <id>   — reopen a closed ticket
'

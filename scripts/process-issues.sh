#!/usr/bin/env bash
# Automated GitHub issue processor for bashkit.
#
# Iterates open issues, processes each one end-to-end:
#   1. Checkout fresh branch from main
#   2. Write reproduction test (failing)
#   3. Implement the fix / feature
#   4. Update specs & threat model if applicable
#   5. Ensure exception/edge-case test coverage
#   6. Run full pre-PR checks
#   7. Create PR, wait for CI green, merge, close issue
#
# Usage:
#   ./scripts/process-issues.sh [options]
#
# Options:
#   --label LABEL     Only process issues with this label (repeatable)
#   --exclude LABEL   Skip issues with this label (repeatable)
#   --limit N         Max issues to process (default: all)
#   --dry-run         Print plan without executing
#   --issue N         Process a single issue by number
#   --skip-merge      Create PRs but don't auto-merge
#   --priority ORDER  Issue sort: created/updated/comments (default: created)
#   --category TYPE   Only process: bug, feat, test, chore, refactor, docs
#   --batch N         Process N issues then pause for review (default: no pause)
#   --continue        Resume after batch pause
#
# Requires: gh, just, cargo, git
# Env: GITHUB_TOKEN (pre-configured in cloud environments)
#
# The script is idempotent — re-running skips already-merged issues.

set -euo pipefail

# --- Colors ---
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# --- Logging ---
log()  { echo -e "${GREEN}[OK]${NC}    $1"; }
info() { echo -e "${BLUE}[INFO]${NC}  $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC}  $1"; }
err()  { echo -e "${RED}[FAIL]${NC}  $1"; }
step() { echo -e "${CYAN}[STEP]${NC}  ${BOLD}$1${NC}"; }

# --- Defaults ---
LABELS=()
EXCLUDE_LABELS=()
LIMIT=0
DRY_RUN=false
SINGLE_ISSUE=""
SKIP_MERGE=false
PRIORITY="created"
CATEGORY=""
BATCH=0
CONTINUE=false
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STATE_DIR="$REPO_ROOT/.issue-processor"
LOG_DIR="$STATE_DIR/logs"
MAIN_BRANCH="main"

# --- Parse args ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --label)      LABELS+=("$2"); shift 2 ;;
        --exclude)    EXCLUDE_LABELS+=("$2"); shift 2 ;;
        --limit)      LIMIT="$2"; shift 2 ;;
        --dry-run)    DRY_RUN=true; shift ;;
        --issue)      SINGLE_ISSUE="$2"; shift 2 ;;
        --skip-merge) SKIP_MERGE=true; shift ;;
        --priority)   PRIORITY="$2"; shift 2 ;;
        --category)   CATEGORY="$2"; shift 2 ;;
        --batch)      BATCH="$2"; shift 2 ;;
        --continue)   CONTINUE=true; shift ;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *) err "Unknown option: $1"; exit 1 ;;
    esac
done

# --- Preflight ---
preflight() {
    local missing=()
    for cmd in gh just cargo git; do
        command -v "$cmd" &>/dev/null || missing+=("$cmd")
    done
    if [[ ${#missing[@]} -gt 0 ]]; then
        err "Missing required tools: ${missing[*]}"
        info "Run: ./scripts/init-cloud-env.sh"
        exit 1
    fi

    # Verify gh auth
    if ! gh auth status &>/dev/null 2>&1; then
        err "gh not authenticated. Set GITHUB_TOKEN or run: gh auth login"
        exit 1
    fi

    # Ensure clean working tree (skip for dry-run)
    if [[ "$DRY_RUN" == "false" ]] && [[ -n "$(git -C "$REPO_ROOT" status --porcelain)" ]]; then
        err "Working tree not clean. Commit or stash changes first."
        git -C "$REPO_ROOT" status --short
        exit 1
    fi

    mkdir -p "$STATE_DIR" "$LOG_DIR"
    log "Preflight passed"
}

# --- Fetch issues ---
fetch_issues() {
    local gh_args=(issue list --state open --json number,title,labels,body --limit 100)

    if [[ -n "$SINGLE_ISSUE" ]]; then
        gh issue view "$SINGLE_ISSUE" --json number,title,labels,body | jq '[.]'
        return
    fi

    for label in "${LABELS[@]+"${LABELS[@]}"}"; do
        gh_args+=(--label "$label")
    done

    local issues
    issues=$(gh "${gh_args[@]}")

    # Filter by exclude labels
    for label in "${EXCLUDE_LABELS[@]+"${EXCLUDE_LABELS[@]}"}"; do
        issues=$(echo "$issues" | jq --arg l "$label" '[.[] | select(.labels | map(.name) | index($l) | not)]')
    done

    # Filter by category (prefix match on title)
    if [[ -n "$CATEGORY" ]]; then
        issues=$(echo "$issues" | jq --arg cat "$CATEGORY" '[.[] | select(.title | startswith($cat + ":"))]')
    fi

    # Sort
    case "$PRIORITY" in
        created)  echo "$issues" | jq 'sort_by(.number)' ;;
        updated)  echo "$issues" | jq 'sort_by(.number) | reverse' ;;
        comments) echo "$issues" | jq 'sort_by(.number)' ;;  # gh doesn't expose comment count in list
        *)        echo "$issues" ;;
    esac
}

# --- Check if issue already processed ---
is_processed() {
    local issue_num="$1"
    [[ -f "$STATE_DIR/done-$issue_num" ]]
}

mark_processed() {
    local issue_num="$1"
    local pr_url="${2:-}"
    echo "$pr_url" > "$STATE_DIR/done-$issue_num"
}

# --- Classify issue ---
classify_issue() {
    local title="$1"
    if [[ "$title" == bug:* ]]; then echo "fix"
    elif [[ "$title" == feat:* ]]; then echo "feat"
    elif [[ "$title" == test:* ]]; then echo "test"
    elif [[ "$title" == chore:* ]]; then echo "chore"
    elif [[ "$title" == refactor:* ]]; then echo "refactor"
    elif [[ "$title" == docs:* ]]; then echo "docs"
    else echo "fix"
    fi
}

# --- Branch name from issue ---
branch_name() {
    local issue_num="$1"
    local title="$2"
    # Sanitize title: lowercase, replace non-alphanum with dash, truncate
    local slug
    slug=$(echo "$title" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]/-/g' | sed 's/--*/-/g' | sed 's/^-//' | sed 's/-$//' | cut -c1-50)
    echo "fix/issue-${issue_num}-${slug}"
}

# --- Determine which specs/areas an issue touches ---
affected_areas() {
    local body="$1"
    local areas=()

    # Check for keywords in issue body
    [[ "$body" =~ parser|lexer|token|AST ]] && areas+=("parser")
    [[ "$body" =~ VFS|filesystem|FileSystem|FsBackend ]] && areas+=("vfs")
    [[ "$body" =~ builtin|echo|awk|grep|sed|jq|curl|tr|cut|sort|uniq|wc|head|tail|find|bc|md5|sha ]] && areas+=("builtins")
    [[ "$body" =~ interpreter|execute|command|pipe|redirect ]] && areas+=("interpreter")
    [[ "$body" =~ tool|Tool|ToolDef|LLM ]] && areas+=("tool")
    [[ "$body" =~ network|HTTP|curl|allowlist ]] && areas+=("network")
    [[ "$body" =~ git|repository ]] && areas+=("git")
    [[ "$body" =~ python|Python|PyO3|Monty ]] && areas+=("python")
    [[ "$body" =~ security|sandbox|escape|inject|threat ]] && areas+=("security")
    [[ "$body" =~ eval|dataset|scoring ]] && areas+=("eval")
    [[ "$body" =~ parallel|Arc|async|thread ]] && areas+=("parallel")

    if [[ ${#areas[@]} -eq 0 ]]; then
        areas+=("general")
    fi

    echo "${areas[*]}"
}

# --- Check if issue touches security-sensitive areas ---
needs_threat_analysis() {
    local body="$1"
    local areas="$2"
    [[ "$areas" =~ security ]] && return 0
    [[ "$areas" =~ parser ]] && return 0
    [[ "$areas" =~ interpreter ]] && return 0
    [[ "$areas" =~ vfs ]] && return 0
    [[ "$areas" =~ network ]] && return 0
    [[ "$areas" =~ git ]] && return 0
    [[ "$body" =~ input|parsing|sandbox|permission|escape|inject ]] && return 0
    return 1
}

# --- Print processing plan for an issue ---
print_plan() {
    local issue_num="$1"
    local title="$2"
    local body="$3"
    local commit_type areas

    commit_type=$(classify_issue "$title")
    areas=$(affected_areas "$body")

    echo ""
    echo -e "${BOLD}═══════════════════════════════════════════════════${NC}"
    echo -e "${BOLD}Issue #${issue_num}: ${title}${NC}"
    echo -e "${BOLD}═══════════════════════════════════════════════════${NC}"
    echo -e "  Type:   ${commit_type}"
    echo -e "  Areas:  ${areas}"
    echo -e "  Branch: $(branch_name "$issue_num" "$title")"
    echo ""
    echo -e "  ${CYAN}Pipeline:${NC}"
    echo -e "    1. Checkout branch from $MAIN_BRANCH"
    echo -e "    2. Write reproduction/regression test (must fail first)"
    echo -e "    3. Implement ${commit_type}"
    if needs_threat_analysis "$body" "$areas"; then
        echo -e "    4. Update threat model (specs/006-threat-model.md)"
    fi
    echo -e "    5. Add exception/edge-case tests"
    echo -e "    6. Update specs if behavior changes"
    echo -e "    7. Run: just pre-pr"
    echo -e "    8. Create PR → wait CI green"
    if [[ "$SKIP_MERGE" == "false" ]]; then
        echo -e "    9. Squash-merge → close issue"
    else
        echo -e "    9. (skip-merge: PR created, manual merge)"
    fi
    echo ""
}

# --- Process a single issue ---
process_issue() {
    local issue_num="$1"
    local title="$2"
    local body="$3"
    local log_file="$LOG_DIR/issue-${issue_num}.log"

    if is_processed "$issue_num"; then
        info "Issue #${issue_num} already processed, skipping"
        return 0
    fi

    local commit_type branch areas
    commit_type=$(classify_issue "$title")
    branch=$(branch_name "$issue_num" "$title")
    areas=$(affected_areas "$body")

    print_plan "$issue_num" "$title" "$body"

    if [[ "$DRY_RUN" == "true" ]]; then
        info "(dry-run) Would process issue #${issue_num}"
        return 0
    fi

    # All output also goes to log
    exec > >(tee -a "$log_file") 2>&1

    step "1/9 Checkout branch"
    git -C "$REPO_ROOT" fetch origin "$MAIN_BRANCH"
    git -C "$REPO_ROOT" checkout -B "$branch" "origin/$MAIN_BRANCH"

    step "2/9 Write reproduction test"
    info "Issue body available in: $log_file"
    info "Affected areas: $areas"
    # The actual test writing is done by the calling agent (Claude).
    # This script provides the framework; the agent fills in code.
    echo "AGENT_ACTION_REQUIRED: write_reproduction_test"
    echo "ISSUE_NUMBER=$issue_num"
    echo "ISSUE_TITLE=$title"
    echo "COMMIT_TYPE=$commit_type"
    echo "AREAS=$areas"
    echo "BODY<<ISSUE_BODY_EOF"
    echo "$body"
    echo "ISSUE_BODY_EOF"

    step "3/9 Implement fix"
    echo "AGENT_ACTION_REQUIRED: implement_fix"

    step "4/9 Threat model analysis"
    if needs_threat_analysis "$body" "$areas"; then
        echo "AGENT_ACTION_REQUIRED: update_threat_model"
        info "Security-sensitive area detected: $areas"
    else
        info "No threat model update needed (areas: $areas)"
    fi

    step "5/9 Exception/edge-case tests"
    echo "AGENT_ACTION_REQUIRED: add_exception_tests"

    step "6/9 Update specs"
    echo "AGENT_ACTION_REQUIRED: update_specs"

    step "7/9 Pre-PR checks"
    (cd "$REPO_ROOT" && just pre-pr)

    step "8/9 Create PR"
    local pr_title="${commit_type}(${areas%% *}): ${title#*: }"
    # Truncate to 70 chars
    pr_title="${pr_title:0:70}"

    local pr_body
    pr_body=$(cat <<PREOF
## Summary

Closes #${issue_num}

${title}

## Changes

<!-- Agent fills this in -->

## Test plan

- [ ] Reproduction test passes
- [ ] Exception/edge-case tests pass
- [ ] \`just pre-pr\` green
- [ ] Specs updated if behavior changed
- [ ] Threat model reviewed if security-sensitive
PREOF
)

    local pr_url
    pr_url=$(cd "$REPO_ROOT" && gh pr create \
        --title "$pr_title" \
        --body "$pr_body" \
        --base "$MAIN_BRANCH" \
        --head "$branch")

    log "PR created: $pr_url"

    step "9/9 Wait for CI and merge"
    if [[ "$SKIP_MERGE" == "true" ]]; then
        info "skip-merge: PR ready at $pr_url"
        mark_processed "$issue_num" "$pr_url"
        return 0
    fi

    # Poll CI status (max 30 min)
    local max_wait=1800
    local waited=0
    local interval=30
    while [[ $waited -lt $max_wait ]]; do
        local checks
        checks=$(gh pr checks "$pr_url" --json name,state 2>/dev/null || echo "[]")
        local pending
        pending=$(echo "$checks" | jq '[.[] | select(.state != "SUCCESS" and .state != "SKIPPED")] | length')
        local failed
        failed=$(echo "$checks" | jq '[.[] | select(.state == "FAILURE")] | length')

        if [[ "$failed" -gt 0 ]]; then
            err "CI failed for #${issue_num}. PR: $pr_url"
            info "Fix failures and re-run, or merge manually."
            mark_processed "$issue_num" "$pr_url"
            return 1
        fi

        if [[ "$pending" -eq 0 ]] && [[ $(echo "$checks" | jq 'length') -gt 0 ]]; then
            log "CI green for #${issue_num}"
            break
        fi

        info "Waiting for CI... (${waited}s / ${max_wait}s)"
        sleep "$interval"
        waited=$((waited + interval))
    done

    if [[ $waited -ge $max_wait ]]; then
        warn "CI timeout for #${issue_num}. PR: $pr_url"
        mark_processed "$issue_num" "$pr_url"
        return 1
    fi

    # Squash merge
    gh pr merge "$pr_url" --squash --delete-branch
    log "Merged and closed: #${issue_num}"

    # Add resolution comment
    gh issue comment "$issue_num" --body "Resolved in $pr_url"

    mark_processed "$issue_num" "$pr_url"
    log "Issue #${issue_num} fully processed"
}

# --- Main ---
main() {
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║       Bashkit Issue Processor                ║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════════════╝${NC}"
    echo ""

    preflight

    step "Fetching open issues..."
    local issues
    issues=$(fetch_issues)

    local total
    total=$(echo "$issues" | jq 'length')
    info "Found $total open issue(s)"

    if [[ "$total" -eq 0 ]]; then
        log "No issues to process"
        exit 0
    fi

    if [[ "$LIMIT" -gt 0 ]]; then
        issues=$(echo "$issues" | jq ".[0:$LIMIT]")
        total=$(echo "$issues" | jq 'length')
        info "Limited to $total issue(s)"
    fi

    local processed=0
    local failed=0
    local skipped=0

    for i in $(seq 0 $((total - 1))); do
        local issue_num title body
        issue_num=$(echo "$issues" | jq -r ".[$i].number")
        title=$(echo "$issues" | jq -r ".[$i].title")
        body=$(echo "$issues" | jq -r ".[$i].body // \"\"")

        if is_processed "$issue_num"; then
            skipped=$((skipped + 1))
            continue
        fi

        if process_issue "$issue_num" "$title" "$body"; then
            processed=$((processed + 1))
        else
            failed=$((failed + 1))
        fi

        # Batch pause
        if [[ "$BATCH" -gt 0 ]] && [[ $((processed + failed)) -ge "$BATCH" ]]; then
            info "Batch limit ($BATCH) reached. Run with --continue to resume."
            break
        fi

        # Return to main branch between issues
        git -C "$REPO_ROOT" checkout "$MAIN_BRANCH" 2>/dev/null || true
    done

    echo ""
    echo -e "${BOLD}═══════════════════════════════════════════════${NC}"
    echo -e "${BOLD}  Results${NC}"
    echo -e "${BOLD}═══════════════════════════════════════════════${NC}"
    echo -e "  Processed: ${GREEN}$processed${NC}"
    echo -e "  Failed:    ${RED}$failed${NC}"
    echo -e "  Skipped:   ${YELLOW}$skipped${NC}"
    echo -e "  Total:     $total"
    echo ""

    if [[ "$failed" -gt 0 ]]; then
        warn "Some issues failed. Check logs in: $LOG_DIR/"
        exit 1
    fi

    log "All done"
}

main "$@"

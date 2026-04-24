#!/usr/bin/env bash
# Fast initialization for cloud agent environments (Claude Code on web, CI, etc.)
# Installs pre-built binaries instead of compiling from source.
#
# Usage: ./scripts/init-cloud-env.sh
#
# This script installs:
# - just: command runner (see justfile)
# - gh: GitHub CLI (for PR/issue operations)
# - doppler: secrets manager CLI

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Decision: pin tool versions + verify SHA-256 in-repo to avoid remote script execution
# and unverified binary extraction during bootstrap.
sha256_file() {
    local file="$1"
    if command -v sha256sum &> /dev/null; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum &> /dev/null; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        error "No SHA-256 tool found (need sha256sum or shasum)"
    fi
}

verify_sha256() {
    local file="$1"
    local expected="$2"
    local actual
    actual=$(sha256_file "$file")
    if [[ "$actual" != "$expected" ]]; then
        error "Checksum mismatch for $(basename "$file"): expected $expected got $actual"
    fi
}

INSTALL_DIR="${HOME}/.cargo/bin"
mkdir -p "$INSTALL_DIR"
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    export PATH="$INSTALL_DIR:$PATH"
fi

install_just() {
    if command -v just &> /dev/null; then
        info "just already installed: $(just --version)"
        return 0
    fi

    info "Installing just (pre-built binary)..."

    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64)  JUST_ARCH="x86_64"; JUST_SHA256="181b91d0ceebe8a57723fb648ed2ce1a44d849438ce2e658339df4f8db5f1263" ;;
        aarch64) JUST_ARCH="aarch64"; JUST_SHA256="d065d0df1a1f99529869fba8a5b3e0a25c1795b9007099b00dfabe29c7c1f7b6" ;;
        *)       error "Unsupported architecture: $ARCH" ;;
    esac

    JUST_VERSION="1.40.0"
    JUST_TARBALL="just-${JUST_VERSION}-${JUST_ARCH}-unknown-linux-musl.tar.gz"
    JUST_URL="https://github.com/casey/just/releases/download/${JUST_VERSION}/${JUST_TARBALL}"

    TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    curl --proto '=https' --tlsv1.2 -sSf --connect-timeout 10 --max-time 60 --retry 2 --retry-delay 2 "$JUST_URL" -o "$TEMP_DIR/$JUST_TARBALL"
    verify_sha256 "$TEMP_DIR/$JUST_TARBALL" "$JUST_SHA256"
    tar -xzf "$TEMP_DIR/$JUST_TARBALL" -C "$TEMP_DIR"
    cp "$TEMP_DIR/just" "$INSTALL_DIR/just"
    chmod +x "$INSTALL_DIR/just"

    if command -v just &> /dev/null; then
        info "just installed: $(just --version)"
    else
        error "Failed to install just"
    fi
}

install_gh() {
    if command -v gh &> /dev/null; then
        info "gh already installed: $(gh --version | head -1)"
        return 0
    fi

    info "Installing gh (GitHub CLI, pre-built binary)..."

    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64)  GH_ARCH="amd64"; GH_SHA256="912fdb1ca29cb005fb746fc5d2b787a289078923a29d0f9ec19a0b00272ded00" ;;
        aarch64) GH_ARCH="arm64"; GH_SHA256="0f31e2a8549c64b5c1679f0b99ce5e0dac7c91da9e86f6246adb8805b0f0b4bb" ;;
        *)       error "Unsupported architecture: $ARCH" ;;
    esac

    # Pinned version — skip GitHub API call to avoid rate limits and hangs
    GH_VERSION="2.63.2"

    GH_TARBALL="gh_${GH_VERSION}_linux_${GH_ARCH}.tar.gz"
    GH_URL="https://github.com/cli/cli/releases/download/v${GH_VERSION}/${GH_TARBALL}"

    TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    info "Downloading gh v${GH_VERSION}..."
    curl -fsSL --connect-timeout 10 --max-time 60 --retry 2 --retry-delay 2 "$GH_URL" -o "$TEMP_DIR/$GH_TARBALL"
    verify_sha256 "$TEMP_DIR/$GH_TARBALL" "$GH_SHA256"
    tar -xzf "$TEMP_DIR/$GH_TARBALL" -C "$TEMP_DIR"
    cp "$TEMP_DIR/gh_${GH_VERSION}_linux_${GH_ARCH}/bin/gh" "$INSTALL_DIR/gh"
    chmod +x "$INSTALL_DIR/gh"

    if command -v gh &> /dev/null; then
        info "gh installed: $(gh --version | head -1)"
    else
        error "Failed to install gh"
    fi
}

install_doppler() {
    if command -v doppler &> /dev/null; then
        info "doppler already installed: $(doppler --version 2>/dev/null)"
        return 0
    fi

    info "Installing Doppler CLI (pre-built binary)..."

    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64)  DOP_ARCH="amd64" ;;
        aarch64) DOP_ARCH="arm64" ;;
        *)       error "Unsupported architecture: $ARCH" ;;
    esac

    # Pinned version — skip GitHub API call to avoid rate limits and hangs
    DOP_VERSION="3.75.2"

    DOP_TARBALL="doppler_${DOP_VERSION}_linux_${DOP_ARCH}.tar.gz"
    DOP_URL="https://github.com/DopplerHQ/cli/releases/download/${DOP_VERSION}/${DOP_TARBALL}"

    TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    info "Downloading doppler v${DOP_VERSION}..."
    curl -fsSL --connect-timeout 10 --max-time 60 --retry 2 --retry-delay 2 "$DOP_URL" -o "$TEMP_DIR/$DOP_TARBALL"
    tar -xzf "$TEMP_DIR/$DOP_TARBALL" -C "$TEMP_DIR"
    cp "$TEMP_DIR/doppler" "$INSTALL_DIR/doppler"
    chmod +x "$INSTALL_DIR/doppler"

    if command -v doppler &> /dev/null; then
        info "doppler installed: $(doppler --version 2>/dev/null)"
    else
        error "Failed to install doppler"
    fi
}

configure_doppler() {
    if [[ -z "${DOPPLER_TOKEN:-}" ]]; then
        warn "DOPPLER_TOKEN not set, skipping Doppler configuration"
        return 0
    fi

    if ! command -v doppler &> /dev/null; then
        warn "doppler not installed, skipping configuration"
        return 0
    fi

    info "Configuring Doppler..."
    doppler setup --no-interactive 2>/dev/null \
        && info "Doppler configured" \
        || warn "Failed to configure Doppler"
}

configure_gh_auth() {
    if ! command -v gh &> /dev/null; then
        warn "gh not installed, skipping GitHub auth check"
        return 0
    fi

    # Prefer Doppler-managed token for non-interactive cloud auth.
    if command -v doppler &> /dev/null && [[ -n "${DOPPLER_TOKEN:-}" ]]; then
        if doppler run -- bash -lc 'GH_TOKEN="$GITHUB_TOKEN" gh auth status >/dev/null 2>&1'; then
            info "gh authenticated via Doppler token"
            return 0
        fi
    fi

    # Fallback: direct environment token (if present).
    if [[ -n "${GITHUB_TOKEN:-}" ]]; then
        if GH_TOKEN="$GITHUB_TOKEN" gh auth status >/dev/null 2>&1; then
            info "gh authenticated via GITHUB_TOKEN"
        else
            warn "GITHUB_TOKEN present but gh auth check failed"
        fi
        return 0
    fi

    warn "gh not authenticated. Set DOPPLER_TOKEN or GITHUB_TOKEN."
}

configure_gh_repo() {
    # Set default repo for gh CLI (needed when git remote uses local proxy)
    local remote_url repo

    remote_url=$(git remote get-url origin 2>/dev/null || echo "")
    if [[ -z "$remote_url" ]]; then
        warn "No git remote found, skipping gh repo configuration"
        return 0
    fi

    # Extract owner/repo from URL patterns:
    # - https://github.com/owner/repo.git
    # - git@github.com:owner/repo.git
    # - http://proxy@127.0.0.1:PORT/git/owner/repo
    if [[ "$remote_url" =~ github\.com[:/]([^/]+/[^/.]+) ]]; then
        repo="${BASH_REMATCH[1]}"
    elif [[ "$remote_url" =~ /git/([^/]+/[^/.]+) ]]; then
        repo="${BASH_REMATCH[1]}"
    else
        warn "Could not extract repo from remote URL: $remote_url"
        return 0
    fi

    repo="${repo%.git}"

    # Add github remote if origin uses proxy
    if [[ ! "$remote_url" =~ github\.com ]]; then
        local github_url="https://github.com/${repo}.git"
        if ! git remote get-url github &>/dev/null; then
            info "Adding 'github' remote: $github_url"
            git remote add github "$github_url"
        fi
        # Fetch main branch
        if ! git rev-parse --verify github/main &>/dev/null; then
            info "Fetching main branch from github remote..."
            git fetch github main 2>/dev/null || warn "Failed to fetch github/main"
        fi
        gh repo set-default "$repo" 2>/dev/null && info "gh default repo set: $repo" || warn "Failed to set default repo"
    else
        gh repo set-default "$repo" 2>/dev/null && info "gh default repo set: $repo" || warn "Failed to set default repo"
    fi
}

main() {
    echo "========================================"
    echo "  Cloud Environment Initialization"
    echo "========================================"
    echo ""

    # Install tools in parallel
    install_just & PID_JUST=$!
    install_gh & PID_GH=$!
    install_doppler & PID_DOPPLER=$!

    INSTALL_FAILED=0
    wait $PID_JUST    || INSTALL_FAILED=1
    wait $PID_GH      || INSTALL_FAILED=1
    wait $PID_DOPPLER || INSTALL_FAILED=1

    if [[ "$INSTALL_FAILED" -eq 1 ]]; then
        error "One or more tool installs failed"
    fi

    configure_gh_repo
    configure_doppler
    configure_gh_auth

    echo ""
    info "Cloud environment ready"
    echo ""
    echo "Installed tools:"
    echo "  - just $(just --version 2>/dev/null || echo '(not in PATH)')"
    echo "  - gh $(gh --version 2>/dev/null | head -1 || echo '(not in PATH)')"
    echo "  - doppler $(doppler --version 2>/dev/null || echo '(not in PATH)')"
    echo ""
    echo "Next steps:"
    echo "  just --list    # See available commands"
    echo "  just build     # Build project"
    echo "  just test      # Run tests"
    echo "========================================"
}

main "$@"

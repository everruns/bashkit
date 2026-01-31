#!/usr/bin/env bash
# Fast initialization for cloud agent environments (Claude Code on web, CI, etc.)
# Installs pre-built binaries instead of compiling from source.
#
# Usage: ./scripts/init-cloud-env.sh
#
# This script installs:
# - just: command runner (see justfile)
# - gh: GitHub CLI (for PR/issue operations)

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

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
    curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to "$INSTALL_DIR"

    if command -v just &> /dev/null; then
        info "just installed: $(just --version)"
    else
        error "Failed to install just"
    fi
}

install_gh() {
    if command -v gh &> /dev/null; then
        info "gh already installed: $(gh --version | grep -m1 '')"
        return 0
    fi

    info "Installing gh (GitHub CLI, pre-built binary)..."

    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64)  GH_ARCH="amd64" ;;
        aarch64) GH_ARCH="arm64" ;;
        *)       error "Unsupported architecture: $ARCH" ;;
    esac

    GH_VERSION=$(curl -sS https://api.github.com/repos/cli/cli/releases/latest | grep '"tag_name"' | cut -d'"' -f4 | sed 's/^v//')
    if [[ -z "$GH_VERSION" ]]; then
        GH_VERSION="2.63.2"
        warn "Could not fetch latest gh version, using fallback: $GH_VERSION"
    fi

    GH_TARBALL="gh_${GH_VERSION}_linux_${GH_ARCH}.tar.gz"
    GH_URL="https://github.com/cli/cli/releases/download/v${GH_VERSION}/${GH_TARBALL}"

    TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    curl -sSL "$GH_URL" -o "$TEMP_DIR/$GH_TARBALL"
    tar -xzf "$TEMP_DIR/$GH_TARBALL" -C "$TEMP_DIR"
    cp "$TEMP_DIR/gh_${GH_VERSION}_linux_${GH_ARCH}/bin/gh" "$INSTALL_DIR/gh"
    chmod +x "$INSTALL_DIR/gh"

    if command -v gh &> /dev/null; then
        info "gh installed: $(gh --version | grep -m1 '')"
    else
        error "Failed to install gh"
    fi
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

    # Check GITHUB_TOKEN
    if [ -z "${GITHUB_TOKEN:-}" ]; then
        warn "GITHUB_TOKEN not set"
    fi

    install_just
    install_gh
    configure_gh_repo

    echo ""
    info "Cloud environment ready"
    echo ""
    echo "Next steps:"
    echo "  just --list    # See available commands"
    echo "  just build     # Build project"
    echo "  just test      # Run tests"
    echo "========================================"
}

main "$@"

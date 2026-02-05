#!/usr/bin/env bash
# Run deepagent sandbox example with uv
# Usage: ./examples/run_deepagent.sh [--demo]

set -e

cd "$(dirname "$0")/.."

# Check for API key
if [ -z "$ANTHROPIC_API_KEY" ]; then
    echo "Error: ANTHROPIC_API_KEY not set"
    echo "  export ANTHROPIC_API_KEY=your_key"
    exit 1
fi

# Create venv if needed
if [ ! -d ".venv" ]; then
    echo "Creating virtual environment..."
    uv venv .venv
fi

# Activate and install dependencies
source .venv/bin/activate

# Install maturin if needed
if ! command -v maturin &> /dev/null; then
    echo "Installing maturin..."
    uv pip install maturin
fi

# Build bashkit
echo "Building bashkit..."
cd crates/bashkit-python
maturin develop --quiet
cd ../..

# Install deepagents dependencies
echo "Installing dependencies..."
uv pip install --quiet "deepagents>=0.3.11" "langchain-anthropic>=0.3"

# Run the example
echo ""
python examples/deepagent_sandbox.py "$@"

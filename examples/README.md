# BashKit Examples

## treasure_hunt_agent.py

LangChain agent that plays a treasure hunt game using BashKit's sandboxed bash tool.

```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/treasure_hunt_agent.py
```

## deepagent_sandbox.py

Deep Agents backend demo with BashKit virtual filesystem. Implements `SandboxBackendProtocol`
providing sandboxed shell execution and file operations through BashKit's VFS.

```bash
export ANTHROPIC_API_KEY=your_key

# Using the runner script (recommended):
./examples/run_deepagent.sh

# Non-interactive demo:
./examples/run_deepagent.sh --demo

# Or manually with uv:
uv venv && source .venv/bin/activate
uv pip install maturin deepagents langchain-anthropic
cd crates/bashkit-python && maturin develop && cd ../..
python examples/deepagent_sandbox.py
```

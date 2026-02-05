# BashKit Examples

## treasure_hunt_agent.py

LangChain agent that plays a treasure hunt game using BashKit's sandboxed bash tool.

```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/treasure_hunt_agent.py
```

## deepagent_sandbox.py

Deep Agents integration with BashKit virtual filesystem. Uses `BashKitBackend` which
implements `SandboxBackendProtocol` providing both shell execution (`execute`) and
file operations (`read_file`, `write_file`, `ls`, `grep`, etc.) through BashKit's VFS.

```bash
# Setup (once)
uv venv && source .venv/bin/activate
uv pip install maturin deepagents langchain-anthropic
cd crates/bashkit-python && maturin develop && cd ../..

# Run
export ANTHROPIC_API_KEY=your_key
python examples/deepagent_sandbox.py
```

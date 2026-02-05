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
export ANTHROPIC_API_KEY=your_key
uv run examples/deepagent_sandbox.py
```

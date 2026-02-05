# BashKit Examples

## treasure_hunt_agent.py

LangChain agent that plays a treasure hunt game using BashKit's sandboxed bash tool.

```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/treasure_hunt_agent.py
```

## deepagent_sandbox.py

Deep Agents integration with BashKit virtual filesystem using both:
- **BashKitBackend**: `SandboxBackendProtocol` for `read_file`, `write_file`, `ls`, etc.
- **BashKitMiddleware**: `AgentMiddleware` adding `bash` tool via `tools` property

Both share the same VFS via `backend.create_middleware()`:

```python
backend = BashKitBackend()
middleware = backend.create_middleware()  # shares VFS
agent = create_deep_agent(backend=backend, middleware=[middleware])
```

Run:
```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/deepagent_sandbox.py
```

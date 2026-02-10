# Bashkit Examples

All examples use [PEP 723](https://peps.python.org/pep-0723/) inline script metadata.
`uv run` resolves dependencies automatically — bashkit installs from PyPI as a pre-built wheel (no Rust toolchain needed).

## treasure_hunt_agent.py

LangChain agent with Bashkit sandbox.

```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/treasure_hunt_agent.py
```

## deepagent_coding_agent.py

Deep Agents with Bashkit middleware + backend.

```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/deepagent_coding_agent.py
```

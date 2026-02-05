# BashKit Examples

## treasure_hunt_agent.py

LangChain agent that plays a treasure hunt game using BashKit's sandboxed bash tool.

```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/treasure_hunt_agent.py
```

## deepagent_sandbox.py

Deep Agents middleware demo with BashKit virtual filesystem. Provides an interactive
sandbox where the agent can execute bash commands, create files, and process data.

```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/deepagent_sandbox.py

# Or run non-interactive demo:
uv run examples/deepagent_sandbox.py --demo
```

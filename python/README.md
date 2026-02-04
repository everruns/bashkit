# BashKit Python Bindings

Python bindings for [BashKit](https://github.com/everruns/bashkit) - a sandboxed bash interpreter for AI agents.

## Features

- **Sandboxed execution**: All commands run in isolation with a virtual filesystem
- **66+ built-in commands**: echo, cat, grep, sed, awk, jq, curl, find, and more
- **Full bash syntax**: Variables, pipelines, redirects, loops, functions, arrays
- **Resource limits**: Protect against infinite loops and runaway scripts
- **LangChain integration**: Ready-to-use tool for LangChain agents

## Installation

```bash
# From PyPI (when published)
pip install bashkit-py

# With LangChain support
pip install 'bashkit-py[langchain]'

# From source
cd python
pip install maturin
maturin develop
```

## Quick Start

```python
import asyncio
from bashkit_py import BashTool

async def main():
    tool = BashTool()

    # Simple command
    result = await tool.execute("echo 'Hello, World!'")
    print(result.stdout)  # Hello, World!

    # Pipeline
    result = await tool.execute("echo -e 'banana\\napple\\ncherry' | sort")
    print(result.stdout)  # apple\nbanana\ncherry

    # Virtual filesystem
    result = await tool.execute("""
        echo 'data' > /tmp/file.txt
        cat /tmp/file.txt
    """)
    print(result.stdout)  # data

asyncio.run(main())
```

## LangChain Integration

```python
from bashkit_py.langchain import create_bash_tool
from langchain_anthropic import ChatAnthropic
from langchain.agents import create_tool_calling_agent, AgentExecutor
from langchain_core.prompts import ChatPromptTemplate

# Create tool
bash_tool = create_bash_tool()

# Create agent
llm = ChatAnthropic(model="claude-sonnet-4-20250514")
prompt = ChatPromptTemplate.from_messages([
    ("system", "You are a helpful assistant with bash skills."),
    ("human", "{input}"),
    ("placeholder", "{agent_scratchpad}"),
])

agent = create_tool_calling_agent(llm, [bash_tool], prompt)
executor = AgentExecutor(agent=agent, tools=[bash_tool])

# Run
result = executor.invoke({"input": "Create a file with today's date"})
```

## Configuration

```python
tool = BashTool(
    username="agent",           # Custom username (whoami)
    hostname="sandbox",         # Custom hostname
    max_commands=1000,          # Limit total commands
    max_loop_iterations=10000,  # Limit loop iterations
)
```

## Synchronous API

```python
from bashkit_py import BashTool

tool = BashTool()
result = tool.execute_sync("echo 'Hello!'")
print(result.stdout)
```

## API Reference

### BashTool

- `execute(commands: str) -> ExecResult`: Execute commands asynchronously
- `execute_sync(commands: str) -> ExecResult`: Execute commands synchronously
- `description() -> str`: Get tool description
- `llmtext() -> str`: Get LLM documentation
- `input_schema() -> str`: Get JSON input schema
- `output_schema() -> str`: Get JSON output schema

### ExecResult

- `stdout: str`: Standard output
- `stderr: str`: Standard error
- `exit_code: int`: Exit code (0 = success)
- `error: Optional[str]`: Error message if execution failed
- `success: bool`: True if exit_code == 0
- `to_dict() -> dict`: Convert to dictionary

## License

MIT

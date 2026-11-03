# Deep Agents

[Deep Agents](https://github.com/langchain-ai/deepagents) is LangChain's agent
harness: planning, subagents, and a set of filesystem and shell tools
(`ls`, `read_file`, `write_file`, `edit_file`, `delete`, `glob`, `grep`,
`execute`) that
run against a pluggable *backend*. `bashkit.deepagents.BashkitBackend` is a
backend that runs all of those tools inside one in-process Bashkit sandbox.

What you get compared to a container or remote sandbox:

- **No infrastructure.** The sandbox is a Python object. No Docker, VM, or API
  key for a sandbox provider; startup takes milliseconds.
- **One shared virtual filesystem.** Files the agent writes with `write_file`
  are visible to `execute`, and the reverse. Nothing touches the host disk.
- **Persistent shell state.** `cd`, `export`, and shell functions carry over
  between `execute` calls.
- **Hard limits.** Command count, loop iterations, and a wall-clock budget the
  model cannot raise.

## Install

Requires Python 3.11+ and Deep Agents 0.7.32 or newer (pulled in by the extra):

```bash
pip install 'bashkit[deepagents]'
```

## Quickstart

```python
from deepagents import create_deep_agent
from bashkit.deepagents import BashkitBackend

backend = BashkitBackend(timeout_seconds=30)

agent = create_deep_agent(
    model="anthropic:claude-sonnet-5",
    backend=backend,
    system_prompt="You are a coding assistant. Use the shell and file tools.",
)

result = agent.invoke(
    {"messages": [{"role": "user", "content": "Write fizzbuzz.sh for 1..15 and run it"}]}
)
print(result["messages"][-1].content)

# The files the agent produced stay in the backend after the run.
print(backend.read("/home/user/fizzbuzz.sh").file_data["content"])
```

The path the agent picks is up to the model; use
`backend.glob("**", "/home/user")` or `backend.ls(...)` to see what it wrote.
The working directory starts at `/home/user`.

A full streaming example lives in
[`examples/deepagent_coding_agent.py`](https://github.com/everruns/bashkit/blob/main/examples/deepagent_coding_agent.py):

```bash
export ANTHROPIC_API_KEY=...
uv run examples/deepagent_coding_agent.py
```

## How to

### Seed files before the run

Put inputs into the sandbox before calling the agent. All of these write to the
same virtual filesystem the agent sees:

```python
backend = BashkitBackend()

# Text files
backend.write("/data/README.md", "# Project\n")

# Raw bytes (images, archives, anything)
backend.upload_files([("/data/logo.png", png_bytes)])

# Or run a setup script
backend.setup("mkdir -p /data/src && echo 'print(1)' > /data/src/main.py")
```

### Read results after the run

```python
text = backend.read("/data/report.md").file_data["content"]
[download] = backend.download_files(["/data/out.tar"])
raw = download.content  # bytes, or None with download.error set
```

### Set limits

The constructor accepts the same sandbox limits as `BashTool`:

```python
backend = BashkitBackend(
    timeout_seconds=30,          # wall-clock budget per execute call
    max_commands=10_000,
    max_loop_iterations=100_000,
    username="agent",
    hostname="sandbox",
    env={"PROJECT": "demo"},
)
```

`timeout_seconds` is a ceiling. Deep Agents lets the model pass a `timeout` on
each `execute` call; Bashkit honors it only when it is shorter. A model asking
for `timeout=600` on a 30-second backend still stops at 30 seconds. A timed-out
command returns a non-zero exit code and a timeout message the model can read.

See [Sandbox configuration & limits](configuration.md) for what each limit
covers.

### Handle large command output

When a command prints more than fits in the model's context, Deep Agents
offloads it. With `BashkitBackend` the full output (stdout and stderr, up to
10 MB) is written to `/large_tool_results/<id>` inside the sandbox, and the
model receives a short head/tail preview that names the file. The model can
then `grep` or `read_file` the parts it needs.

The capture happens as the command runs, so it is not limited by Bashkit's
1 MB in-memory stdout cap, and it does not wrap the command in a subshell, so
`cd` and `export` still persist. No configuration is needed.

### Let the model see images and PDFs

`read_file` on a binary file returns base64 content (up to 500 KB) that
multimodal models can view. Upload the file, then ask the agent about it:

```python
backend.upload_files([("/data/chart.png", png_bytes)])
agent.invoke({"messages": [{"role": "user", "content": "Describe /data/chart.png"}]})
```

### Enable embedded Python or TypeScript

Bashkit ships embedded interpreters that run inside the sandbox. They are off
by default:

```python
backend = BashkitBackend(
    python=True,
    typescript=True,
    env={"BASHKIT_ALLOW_INPROCESS_PYTHON": "1"},  # required with python=True
)
```

See the [Python builtin](../crates/bashkit/docs/python.md) and
[TypeScript builtin](builtin_typescript.md) guides for what each supports.

### Expose host functions to scripts

`external_functions` and `external_handler` let in-sandbox Python call back into
your application (for example, to query a database the agent should not reach
directly). The arguments are the same as on `BashTool`; see the
[Python package README](../crates/bashkit-python/README.md).

### Add Bashkit's own `bashkit` tool (optional)

The backend already gives the agent Deep Agents' `execute` tool. If you also
want Bashkit's `bashkit` tool, whose description lists the sandbox's commands and
limits, add the middleware. It shares the backend's filesystem and shell state:

```python
backend = BashkitBackend()
agent = create_deep_agent(
    model="anthropic:claude-sonnet-5",
    backend=backend,
    middleware=[backend.create_middleware()],
)
```

### Async

`agent.ainvoke(...)` and `agent.astream_events(...)` work unchanged. The
backend implements the async protocol methods natively, so callbacks run on
your event loop.

### Isolate users and sessions

Each `BashkitBackend` is a separate sandbox with its own filesystem and shell
state. Create one per user or conversation. `backend.reset()` clears the
filesystem and shell state of an existing one.

## Limits and differences from container sandboxes

- **Not a Linux machine.** Commands are Bashkit's builtins (coreutils, `grep`,
  `sed`, `awk`, `jq`, `git`, and more), not host binaries. `apt`, `pip install`,
  and native executables are not available. See
  [Compatibility](../crates/bashkit/docs/compatibility.md).
- **No network by default.** `curl` and `wget` are denied unless network access
  is configured, and `BashkitBackend` does not expose a network option yet.
- **Per-call file operations are bounded.** `glob` and `grep` stop after 10,000
  files or 10 MB scanned, and `grep` returns at most 1,000 matches.

## See also

- [LLM tools](llm-tools.md), `BashTool` for other agent frameworks
- [Get started with Python](start-python.md)
- [Security](security.md), the sandbox boundary and threat model
- [Virtual filesystem](filesystem.md)

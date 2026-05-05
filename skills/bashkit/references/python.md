# Python API

Use Python examples when the user wants Bashkit from Python code or Python agent frameworks.

## Install

```bash
pip install bashkit
```

Optional integrations:

```bash
pip install 'bashkit[langchain]'
pip install 'bashkit[pydantic-ai]'
pip install 'bashkit[deepagents]'
```

## Sync Execution

```python
from bashkit import Bash

bash = Bash()

result = bash.execute_sync("echo 'Hello, World!'")
print(result.stdout)

bash.execute_sync("export APP_ENV=dev")
print(bash.execute_sync("echo $APP_ENV").stdout)
```

## Async Execution

```python
import asyncio
from bashkit import Bash


async def main():
    bash = Bash()

    result = await bash.execute("printf 'banana\\napple\\ncherry\\n' | sort")
    print(result.stdout)

    await bash.execute("printf 'data\\n' > /tmp/file.txt")
    saved = await bash.execute("cat /tmp/file.txt")
    print(saved.stdout)


asyncio.run(main())
```

## Virtual Filesystem

```python
from bashkit import Bash

bash = Bash()
bash.mkdir("/data", recursive=True)
bash.write_file("/data/config.json", '{"debug": true}\n')

print(bash.read_file("/data/config.json"))
print(bash.execute_sync("cat /data/config.json").stdout)
```

## Host Mounts

```python
from bashkit import Bash

bash = Bash(mounts=[
    {"host_path": "/path/to/data", "vfs_path": "/data"},
    {"host_path": "/path/to/workspace", "vfs_path": "/workspace", "writable": True},
])

print(bash.execute_sync("ls /workspace").stdout)
```

## Network Allowlist

```python
from bashkit import Bash

bash = Bash(network={"allow": ["https://api.github.com"]})
trusted = Bash(network={"allow_all": True})
```

## Agent Tool Wrapper

```python
from bashkit import BashTool

tool = BashTool()
result = tool.execute_sync("echo 'Hello, World!'")
print(result.stdout)
```

## Reference

- PyPI: https://pypi.org/project/bashkit/
- Python package docs: https://github.com/everruns/bashkit/tree/main/crates/bashkit-python

"""
Bashkit server: persistent process that receives bash commands via JSON-line protocol
and executes them in bashkit's virtual bash interpreter with virtual filesystem.

Protocol (stdin/stdout, one JSON object per line):
  Request:  {"id": "...", "command": "echo hello"}
  Response: {"id": "...", "stdout": "hello\n", "stderr": "", "exit_code": 0}
  Ready:    {"ready": true} (sent on startup)
"""

import json
import sys
import asyncio

from bashkit import BashTool


async def main():
    tool = BashTool(
        username="user",
        hostname="pi-sandbox",
        max_commands=50000,
        max_loop_iterations=100000,
    )

    # Signal ready
    sys.stdout.write(json.dumps({"ready": True}) + "\n")
    sys.stdout.flush()

    loop = asyncio.get_event_loop()
    reader = asyncio.StreamReader()
    protocol = asyncio.StreamReaderProtocol(reader)
    await loop.connect_read_pipe(lambda: protocol, sys.stdin)

    while True:
        line = await reader.readline()
        if not line:
            break

        try:
            req = json.loads(line.decode("utf-8").strip())
        except json.JSONDecodeError:
            continue

        req_id = req.get("id", "")
        command = req.get("command", "")
        timeout_ms = req.get("timeout_ms")

        if req.get("type") == "reset":
            tool.reset()
            resp = {"id": req_id, "stdout": "", "stderr": "", "exit_code": 0}
        else:
            result = await tool.execute(command)
            resp = {
                "id": req_id,
                "stdout": result.stdout,
                "stderr": result.stderr,
                "exit_code": result.exit_code,
            }
            if result.error:
                resp["error"] = result.error

        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    asyncio.run(main())

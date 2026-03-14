# In-process bashkit-python benchmark runner
# Protocol: read JSON lines from stdin, write JSON lines to stdout
# Each request: {"script": "..."}
# Each response: {"stdout": "...", "stderr": "...", "exitCode": 0}
# Sends {"ready": true} on startup

import json
import sys

try:
    from bashkit import Bash
except ImportError:
    sys.stderr.write("bashkit python package not available\n")
    sys.exit(1)

# Signal ready
sys.stdout.write(json.dumps({"ready": True}) + "\n")
sys.stdout.flush()

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
        script = req["script"]
        bash = Bash()
        result = bash.execute_sync(script)
        resp = {
            "stdout": result.stdout or "",
            "stderr": result.stderr or "",
            "exitCode": result.exit_code,
        }
    except Exception as e:
        resp = {"stdout": "", "stderr": str(e), "exitCode": 1}
    sys.stdout.write(json.dumps(resp) + "\n")
    sys.stdout.flush()

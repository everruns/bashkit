# Persistent gbash server benchmark runner
# Starts gbash --server on a Unix socket, bridges JSON-RPC to the
# stdin/stdout JSON lines protocol used by PersistentChild.
#
# Each benchmark creates a fresh session for isolation.
#
# Protocol: read JSON lines from stdin, write JSON lines to stdout
# Each request: {"script": "..."}
# Each response: {"stdout": "...", "stderr": "...", "exitCode": 0}
# Sends {"ready": true} on startup

import json
import os
import socket
import subprocess
import sys
import tempfile
import time


def find_gbash():
    home = os.environ.get("HOME", "")
    candidates = [
        os.path.join(home, "go/bin/gbash"),
        os.path.join(os.environ.get("GOPATH", ""), "bin/gbash"),
        "/usr/local/bin/gbash",
        "/usr/bin/gbash",
    ]
    for path in candidates:
        if path and os.path.isfile(path):
            return path
    # Try PATH
    import shutil

    found = shutil.which("gbash")
    if found:
        return found
    return None


def rpc_call(sock, method, params=None):
    req = {"jsonrpc": "2.0", "method": method, "id": 1}
    if params:
        req["params"] = params
    sock.sendall((json.dumps(req) + "\n").encode())
    data = b""
    while b"\n" not in data:
        chunk = sock.recv(65536)
        if not chunk:
            raise ConnectionError("gbash server closed connection")
        data += chunk
    return json.loads(data.decode())


def main():
    gbash_path = find_gbash()
    if not gbash_path:
        sys.stderr.write("gbash not found\n")
        sys.exit(1)

    sock_path = os.path.join(tempfile.mkdtemp(), "gbash-bench.sock")

    proc = subprocess.Popen(
        [gbash_path, "--server", "--socket", sock_path, "--session-ttl", "10m"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    # Wait for socket to appear
    for _ in range(50):
        if os.path.exists(sock_path):
            break
        time.sleep(0.1)
    else:
        sys.stderr.write("gbash server did not start\n")
        proc.kill()
        sys.exit(1)

    # Signal ready
    sys.stdout.write(json.dumps({"ready": True}) + "\n")
    sys.stdout.flush()

    try:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                req = json.loads(line)
                script = req["script"]

                sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                sock.connect(sock_path)

                # Create session
                create_resp = rpc_call(sock, "session.create")
                session_id = create_resp["result"]["session"]["session_id"]

                # Execute script
                exec_resp = rpc_call(
                    sock,
                    "session.exec",
                    {"session_id": session_id, "script": script},
                )
                result = exec_resp.get("result", {})

                resp = {
                    "stdout": result.get("stdout", ""),
                    "stderr": result.get("stderr", ""),
                    "exitCode": result.get("exit_code", -1),
                }

                sock.close()
            except Exception as e:
                resp = {"stdout": "", "stderr": str(e), "exitCode": 1}

            sys.stdout.write(json.dumps(resp) + "\n")
            sys.stdout.flush()
    finally:
        proc.terminate()
        proc.wait()
        try:
            os.unlink(sock_path)
            os.rmdir(os.path.dirname(sock_path))
        except OSError:
            pass


if __name__ == "__main__":
    main()

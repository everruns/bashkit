"""Python network-config parity tests for Bash and BashTool."""

# Decision: use a loopback HTTP server so tests stay hermetic and never depend
# on public internet reachability.
# Decision: step-1 coverage exercises both omitted-network vs explicit-empty
# allowlist semantics because those two cases intentionally differ.

import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from bashkit import Bash, BashTool


class _OkHandler(BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802 - stdlib handler name
        body = b"ok\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):  # noqa: A003 - stdlib API name
        return


@pytest.fixture
def loopback_server():
    server = ThreadingHTTPServer(("127.0.0.1", 0), _OkHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        host, port = server.server_address
        yield f"http://{host}:{port}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_network_disabled_by_default(factory, loopback_server):
    shell = factory()

    result = shell.execute_sync(f"curl -s {loopback_server}")

    assert result.exit_code != 0
    assert "network access not configured" in result.stderr


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_network_explicit_empty_allowlist_blocks_all(factory, loopback_server):
    shell = factory(network={"allow": []})

    result = shell.execute_sync(f"curl -s {loopback_server}")

    assert result.exit_code != 0
    assert "empty allowlist" in result.stderr


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
@pytest.mark.parametrize(
    ("label", "command"),
    [
        ("curl", "curl -s {url}"),
        ("wget", "wget -q -O - {url}"),
        ("http", "http {url}"),
    ],
)
def test_network_allowlisted_requests_succeed(factory, loopback_server, label, command):
    shell = factory(
        network={"allow": [loopback_server], "block_private_ips": False},
    )

    result = shell.execute_sync(command.format(url=loopback_server))

    assert result.exit_code == 0, f"{label} failed: {result.stderr}"
    assert result.stdout.strip() == "ok"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_network_allow_all_mode(factory, loopback_server):
    shell = factory(network={"allow_all": True, "block_private_ips": False})

    result = shell.execute_sync(f"curl -s {loopback_server}")

    assert result.exit_code == 0
    assert result.stdout.strip() == "ok"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_network_non_allowlisted_url_still_fails(factory, loopback_server):
    shell = factory(
        network={"allow": [loopback_server], "block_private_ips": False},
    )
    blocked_url = loopback_server.rsplit(":", 1)[0] + ":1"

    result = shell.execute_sync(f"curl -s {blocked_url}")

    assert result.exit_code != 0
    assert "allowlist" in result.stderr.lower()


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_network_blocks_private_ips_by_default(factory, loopback_server):
    shell = factory(network={"allow": [loopback_server]})

    result = shell.execute_sync(f"curl -s {loopback_server}")

    assert result.exit_code != 0
    assert "private/reserved IP" in result.stderr


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_network_reset_preserves_config(factory, loopback_server):
    shell = factory(
        network={"allow": [loopback_server], "block_private_ips": False},
    )

    before = shell.execute_sync(f"curl -s {loopback_server}")
    shell.reset()
    after = shell.execute_sync(f"curl -s {loopback_server}")

    assert before.exit_code == 0
    assert before.stdout.strip() == "ok"
    assert after.exit_code == 0
    assert after.stdout.strip() == "ok"


@pytest.mark.parametrize("factory", [Bash, BashTool], ids=["bash", "bash_tool"])
def test_network_from_snapshot_accepts_config(factory, loopback_server):
    snapshot = factory().snapshot()

    restored = factory.from_snapshot(
        snapshot,
        network={"allow": [loopback_server], "block_private_ips": False},
    )
    result = restored.execute_sync(f"curl -s {loopback_server}")

    assert result.exit_code == 0
    assert result.stdout.strip() == "ok"

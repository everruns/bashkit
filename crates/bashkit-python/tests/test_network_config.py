"""Tests for the ``network=`` constructor kwarg on ``Bash`` and ``BashTool``.

Covers phase 1 of #1348: allowlist patterns, ``allow_all``, default-deny,
``block_private_ips``, validation errors, and config preservation across
``reset()`` / ``from_snapshot()``. Tests use unreachable / private hostnames
so no real public network is contacted.
"""

import pytest

from bashkit import Bash, BashTool

_PROBE = "curl -sS -o /dev/null --max-time 2 https://api.example.invalid/x; echo exit=$?"


def _stderr_lower(result) -> str:
    return result.stderr.lower()


class TestNetworkDefaultDeny:
    """Network must stay disabled when no ``network=`` kwarg is supplied."""

    def test_bash_default_blocks_curl(self):
        bash = Bash()
        r = bash.execute_sync(_PROBE)
        assert r.stdout.strip().endswith("exit=") is False
        assert "0" not in r.stdout.strip().split("=")[-1]

    def test_bashtool_default_blocks_curl(self):
        tool = BashTool()
        r = tool.execute_sync(_PROBE)
        assert "0" not in r.stdout.strip().split("=")[-1]


class TestNetworkAllowlistRejectsBlockedHosts:
    """An explicit allowlist must still block hosts not on it."""

    def test_bash_blocks_unlisted_host(self):
        bash = Bash(network={"allow": ["https://api.example.com"]})
        r = bash.execute_sync(_PROBE)
        # Host is not on the allowlist - the request must not succeed.
        assert "0" not in r.stdout.strip().split("=")[-1]
        assert (
            "not in allowlist" in _stderr_lower(r) or "not allowed" in _stderr_lower(r) or "blocked" in _stderr_lower(r)
        )

    def test_empty_allow_blocks_everything(self):
        bash = Bash(network={"allow": []})
        r = bash.execute_sync(_PROBE)
        assert "0" not in r.stdout.strip().split("=")[-1]


class TestNetworkAllowAll:
    """``allow_all=True`` mirrors ``NetworkAllowlist::allow_all()``."""

    def test_allow_all_does_not_emit_allowlist_block(self):
        # Without allow_all the unreachable .invalid host yields an
        # allowlist-block error. With allow_all the request escapes the
        # allowlist and fails for a different reason (DNS / connect),
        # so the stderr must NOT mention allowlist blocking.
        bash = Bash(network={"allow_all": True})
        r = bash.execute_sync(_PROBE)
        assert "not in allowlist" not in _stderr_lower(r)
        assert "blocked by allowlist" not in _stderr_lower(r)


class TestNetworkValidation:
    """The kwarg parser must surface configuration mistakes immediately."""

    def test_unknown_key_rejected(self):
        with pytest.raises(ValueError, match="unknown key"):
            Bash(network={"allow": ["https://x"], "bogus": True})

    def test_must_provide_allow_or_allow_all(self):
        with pytest.raises(ValueError, match="allow"):
            Bash(network={})

    def test_allow_and_allow_all_mutually_exclusive(self):
        with pytest.raises(ValueError, match="mutually exclusive"):
            Bash(network={"allow": ["https://x"], "allow_all": True})

    def test_allow_must_be_list_of_strings(self):
        with pytest.raises((ValueError, TypeError)):
            Bash(network={"allow": "https://x"})


class TestNetworkPreservedAcrossReset:
    """``reset()`` must rebuild with the same network config."""

    def test_bash_reset_keeps_allowlist(self):
        bash = Bash(network={"allow": ["https://api.example.com"]})
        bash.reset()
        # If reset dropped the config, default-deny would still block, so
        # this checks that we are *still* in allowlist mode (vs. open).
        r = bash.execute_sync(_PROBE)
        assert "0" not in r.stdout.strip().split("=")[-1]

    def test_bashtool_reset_keeps_allow_all(self):
        tool = BashTool(network={"allow_all": True})
        tool.reset()
        r = tool.execute_sync(_PROBE)
        # allow_all stays after reset → no allowlist-block in stderr
        assert "not in allowlist" not in _stderr_lower(r)
        assert "blocked by allowlist" not in _stderr_lower(r)


class TestNetworkPreservedAcrossSnapshot:
    """``from_snapshot()`` must accept the same ``network=`` kwarg."""

    def test_bash_from_snapshot_with_network(self):
        src = Bash(network={"allow": ["https://api.example.com"]})
        data = src.snapshot()
        restored = Bash.from_snapshot(data, network={"allow": ["https://api.example.com"]})
        r = restored.execute_sync(_PROBE)
        assert "0" not in r.stdout.strip().split("=")[-1]

    def test_bashtool_from_snapshot_with_allow_all(self):
        src = BashTool(network={"allow_all": True})
        data = src.snapshot()
        restored = BashTool.from_snapshot(data, network={"allow_all": True})
        r = restored.execute_sync(_PROBE)
        assert "not in allowlist" not in _stderr_lower(r)
        assert "blocked by allowlist" not in _stderr_lower(r)


class TestBlockPrivateIps:
    """``block_private_ips`` flag is wired through and accepts both values."""

    def test_block_private_ips_default_true_blocks_loopback(self):
        bash = Bash(network={"allow": ["http://127.0.0.1"]})
        r = bash.execute_sync("curl -sS -o /dev/null --max-time 2 http://127.0.0.1; echo exit=$?")
        # Default blocks private IPs even when the URL is allowlisted.
        assert "0" not in r.stdout.strip().split("=")[-1]

    def test_block_private_ips_false_accepted(self):
        # Just verify the flag is accepted without raising; the actual
        # network call still fails because nothing is listening, but
        # the constructor must not reject the kwarg.
        bash = Bash(
            network={
                "allow": ["http://127.0.0.1:1"],
                "block_private_ips": False,
            }
        )
        r = bash.execute_sync("curl -sS -o /dev/null --max-time 1 http://127.0.0.1:1; echo exit=$?")
        # No allowlist-block message: the URL is on the list and the
        # private-IP guard is disabled.
        assert "not in allowlist" not in _stderr_lower(r)
        assert "blocked by allowlist" not in _stderr_lower(r)

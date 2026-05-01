"""Tests for ``credentials`` and ``credential_placeholders`` on ``network=``.

Covers phase 2 of #1348: per-host credential injection (script unaware) and
placeholder-mode injection (script sees an opaque env-var placeholder that
the runtime replaces on the wire). Tests do not contact any public network;
they exercise validation, env-var visibility, and config preservation across
``reset()`` / ``from_snapshot()``.
"""

import pytest

from bashkit import Bash, BashTool

_PROBE = "curl -sS -o /dev/null --max-time 2 https://api.example.invalid/x; echo exit=$?"


class TestCredentialValidation:
    """The parser surfaces credential-config mistakes immediately."""

    def test_credentials_must_be_list(self):
        with pytest.raises(ValueError, match="credentials"):
            Bash(network={"allow": ["https://x"], "credentials": {"pattern": "x"}})

    def test_credentials_entry_must_be_dict(self):
        with pytest.raises(ValueError, match="credentials"):
            Bash(network={"allow": ["https://x"], "credentials": ["not-a-dict"]})

    def test_credentials_missing_pattern(self):
        with pytest.raises(ValueError, match="pattern"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credentials": [{"kind": "bearer", "token": "t"}],
                }
            )

    def test_credentials_missing_kind(self):
        with pytest.raises(ValueError, match="kind"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credentials": [{"pattern": "https://x", "token": "t"}],
                }
            )

    def test_credentials_unknown_kind(self):
        with pytest.raises(ValueError, match="kind"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credentials": [{"pattern": "https://x", "kind": "basic", "token": "t"}],
                }
            )

    def test_bearer_missing_token(self):
        with pytest.raises(ValueError, match="token"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credentials": [{"pattern": "https://x", "kind": "bearer"}],
                }
            )

    def test_header_missing_name(self):
        with pytest.raises(ValueError, match="name"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credentials": [{"pattern": "https://x", "kind": "header", "value": "v"}],
                }
            )

    def test_header_empty_name(self):
        with pytest.raises(ValueError, match="name"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credentials": [
                        {
                            "pattern": "https://x",
                            "kind": "header",
                            "name": "",
                            "value": "v",
                        }
                    ],
                }
            )

    def test_headers_must_be_list_of_pairs(self):
        with pytest.raises(ValueError, match="headers"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credentials": [
                        {
                            "pattern": "https://x",
                            "kind": "headers",
                            "headers": "X-Api-Key:value",
                        }
                    ],
                }
            )

    def test_headers_empty_list_rejected(self):
        with pytest.raises(ValueError, match="at least one"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credentials": [{"pattern": "https://x", "kind": "headers", "headers": []}],
                }
            )

    def test_credential_unknown_extra_key(self):
        with pytest.raises(ValueError, match="unknown key"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credentials": [
                        {
                            "pattern": "https://x",
                            "kind": "bearer",
                            "token": "t",
                            "extra": 1,
                        }
                    ],
                }
            )

    def test_placeholder_missing_env(self):
        with pytest.raises(ValueError, match="env"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credential_placeholders": [{"pattern": "https://x", "kind": "bearer", "token": "t"}],
                }
            )

    def test_placeholder_empty_env(self):
        with pytest.raises(ValueError, match="env"):
            Bash(
                network={
                    "allow": ["https://x"],
                    "credential_placeholders": [
                        {
                            "env": "",
                            "pattern": "https://x",
                            "kind": "bearer",
                            "token": "t",
                        }
                    ],
                }
            )


class TestCredentialsAccepted:
    """Constructor accepts well-formed credentials without raising."""

    def test_bearer_injection_accepted_on_bash(self):
        Bash(
            network={
                "allow": ["https://api.github.com"],
                "credentials": [
                    {
                        "pattern": "https://api.github.com",
                        "kind": "bearer",
                        "token": "ghp_xxx",
                    }
                ],
            }
        )

    def test_bearer_injection_accepted_on_bashtool(self):
        BashTool(
            network={
                "allow": ["https://api.github.com"],
                "credentials": [
                    {
                        "pattern": "https://api.github.com",
                        "kind": "bearer",
                        "token": "ghp_xxx",
                    }
                ],
            }
        )

    def test_header_injection_accepted(self):
        Bash(
            network={
                "allow": ["https://api.example.com"],
                "credentials": [
                    {
                        "pattern": "https://api.example.com",
                        "kind": "header",
                        "name": "X-Api-Key",
                        "value": "secret",
                    }
                ],
            }
        )

    def test_headers_injection_accepted_with_tuples(self):
        Bash(
            network={
                "allow": ["https://api.example.com"],
                "credentials": [
                    {
                        "pattern": "https://api.example.com",
                        "kind": "headers",
                        "headers": [("X-Api-Key", "k"), ("X-Api-Secret", "s")],
                    }
                ],
            }
        )

    def test_headers_injection_accepted_with_lists(self):
        Bash(
            network={
                "allow": ["https://api.example.com"],
                "credentials": [
                    {
                        "pattern": "https://api.example.com",
                        "kind": "headers",
                        "headers": [["X-Api-Key", "k"]],
                    }
                ],
            }
        )


class TestCredentialPlaceholders:
    """Placeholder mode exposes opaque env vars to scripts."""

    def test_placeholder_env_var_is_set(self):
        bash = Bash(
            network={
                "allow": ["https://api.openai.com"],
                "credential_placeholders": [
                    {
                        "env": "OPENAI_API_KEY",
                        "pattern": "https://api.openai.com",
                        "kind": "bearer",
                        "token": "sk-real-key",
                    }
                ],
            }
        )
        r = bash.execute_sync("echo $OPENAI_API_KEY")
        assert r.exit_code == 0
        out = r.stdout.strip()
        # Env var must contain the runtime-generated placeholder, never the
        # real secret. Format: `bk_placeholder_<32 hex chars>`.
        assert out.startswith("bk_placeholder_")
        assert "sk-real-key" not in r.stdout

    def test_placeholder_does_not_leak_secret_via_env_dump(self):
        bash = Bash(
            network={
                "allow": ["https://api.openai.com"],
                "credential_placeholders": [
                    {
                        "env": "OPENAI_API_KEY",
                        "pattern": "https://api.openai.com",
                        "kind": "bearer",
                        "token": "sk-real-key",
                    }
                ],
            }
        )
        r = bash.execute_sync("env")
        assert "sk-real-key" not in r.stdout
        assert "bk_placeholder_" in r.stdout

    def test_placeholder_env_var_on_bashtool(self):
        tool = BashTool(
            network={
                "allow": ["https://api.openai.com"],
                "credential_placeholders": [
                    {
                        "env": "OPENAI_API_KEY",
                        "pattern": "https://api.openai.com",
                        "kind": "bearer",
                        "token": "sk-real-key",
                    }
                ],
            }
        )
        r = tool.execute_sync("echo $OPENAI_API_KEY")
        assert r.exit_code == 0
        assert r.stdout.strip().startswith("bk_placeholder_")


class TestCredentialsPreservedAcrossReset:
    """``reset()`` rebuilds with the same credential surface."""

    def test_reset_preserves_placeholder_env_var(self):
        bash = Bash(
            network={
                "allow": ["https://api.openai.com"],
                "credential_placeholders": [
                    {
                        "env": "OPENAI_API_KEY",
                        "pattern": "https://api.openai.com",
                        "kind": "bearer",
                        "token": "sk-real-key",
                    }
                ],
            }
        )
        bash.reset()
        # After reset the env var must still be a placeholder (the actual
        # value is regenerated, but the variable must remain present).
        r = bash.execute_sync("echo $OPENAI_API_KEY")
        assert r.exit_code == 0
        assert r.stdout.strip().startswith("bk_placeholder_")

    def test_reset_preserves_injection_does_not_open_allowlist(self):
        # Injection alone does not relax the allowlist — unlisted hosts
        # must still be blocked even when credentials are configured.
        bash = Bash(
            network={
                "allow": ["https://api.github.com"],
                "credentials": [
                    {
                        "pattern": "https://api.github.com",
                        "kind": "bearer",
                        "token": "ghp_xxx",
                    }
                ],
            }
        )
        bash.reset()
        r = bash.execute_sync(_PROBE)
        assert "0" not in r.stdout.strip().split("=")[-1]


class TestCredentialsPreservedAcrossSnapshot:
    """``from_snapshot()`` accepts the same credential surface."""

    def test_bash_from_snapshot_with_credentials(self):
        src = Bash(
            network={
                "allow": ["https://api.github.com"],
                "credentials": [
                    {
                        "pattern": "https://api.github.com",
                        "kind": "bearer",
                        "token": "ghp_xxx",
                    }
                ],
            }
        )
        data = src.snapshot()
        restored = Bash.from_snapshot(
            data,
            network={
                "allow": ["https://api.github.com"],
                "credentials": [
                    {
                        "pattern": "https://api.github.com",
                        "kind": "bearer",
                        "token": "ghp_xxx",
                    }
                ],
            },
        )
        # Sanity: still allowlisted, still default-secure for unlisted hosts.
        r = restored.execute_sync(_PROBE)
        assert "0" not in r.stdout.strip().split("=")[-1]

    def test_bashtool_from_snapshot_with_placeholder(self):
        src = BashTool(
            network={
                "allow": ["https://api.openai.com"],
                "credential_placeholders": [
                    {
                        "env": "OPENAI_API_KEY",
                        "pattern": "https://api.openai.com",
                        "kind": "bearer",
                        "token": "sk-real-key",
                    }
                ],
            }
        )
        data = src.snapshot()
        restored = BashTool.from_snapshot(
            data,
            network={
                "allow": ["https://api.openai.com"],
                "credential_placeholders": [
                    {
                        "env": "OPENAI_API_KEY",
                        "pattern": "https://api.openai.com",
                        "kind": "bearer",
                        "token": "sk-real-key",
                    }
                ],
            },
        )
        r = restored.execute_sync("echo $OPENAI_API_KEY")
        assert r.exit_code == 0
        assert r.stdout.strip().startswith("bk_placeholder_")

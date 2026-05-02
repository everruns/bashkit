"""Tests for the embedded SQLite (Turso) builtin exposed via the
`sqlite=True` constructor flag on `Bash`.

Mirrors `tests/test_python_security.py`'s structure: opt-in gate,
positive flow, schema persistence across calls, dot-commands, output
formatting, and the security policies (ATTACH/DETACH and PRAGMA deny
list).
"""

import pytest

from bashkit import Bash


class TestOptIn:
    """The builtin is gated at the binding layer behind `sqlite=True`."""

    def test_default_disabled(self):
        bash = Bash()
        r = bash.execute_sync("sqlite :memory: 'SELECT 1'")
        # Without sqlite=True the builtin is not registered, so bash
        # treats it as an external command (not found).
        assert "command not found" in r.stderr

    def test_enable_via_constructor(self):
        bash = Bash(sqlite=True)
        r = bash.execute_sync("sqlite :memory: 'SELECT 1 + 2'")
        assert r.exit_code == 0, f"stderr was: {r.stderr}"
        assert r.stdout.strip() == "3"


class TestBasicQueries:
    @pytest.fixture
    def bash(self):
        return Bash(sqlite=True)

    def test_create_insert_select_round_trip(self, bash):
        r = bash.execute_sync(
            "sqlite :memory: 'CREATE TABLE t(a INTEGER, b TEXT); "
            'INSERT INTO t VALUES (1, "x"), (2, "y"); '
            "SELECT * FROM t ORDER BY a'"
        )
        assert r.exit_code == 0, f"stderr was: {r.stderr}"
        assert r.stdout == "1|x\n2|y\n"

    def test_header_flag(self, bash):
        r = bash.execute_sync(
            "sqlite -header :memory: 'CREATE TABLE t(x, y); INSERT INTO t VALUES (1, 2); SELECT * FROM t'"
        )
        assert r.exit_code == 0, f"stderr was: {r.stderr}"
        assert r.stdout == "x|y\n1|2\n"

    def test_csv_mode(self, bash):
        r = bash.execute_sync("sqlite -csv :memory: 'SELECT \"hello,world\"'")
        assert r.exit_code == 0, f"stderr was: {r.stderr}"
        # CSV quoting: comma inside the field forces double quotes.
        assert r.stdout.strip() == '"hello,world"'

    def test_json_mode(self, bash):
        import json

        r = bash.execute_sync("sqlite -json :memory: 'SELECT 1 AS i, \"hi\" AS s'")
        assert r.exit_code == 0, f"stderr was: {r.stderr}"
        parsed = json.loads(r.stdout.strip())
        assert parsed[0]["i"] == 1
        assert parsed[0]["s"] == "hi"


class TestVfsPersistence:
    """Database files persist on the bashkit VFS across `execute()` calls."""

    def test_round_trip(self):
        bash = Bash(sqlite=True)
        seed = bash.execute_sync(
            "sqlite /tmp/notes.sqlite 'CREATE TABLE notes(body TEXT); INSERT INTO notes VALUES (\"hello\")'"
        )
        assert seed.exit_code == 0, f"stderr was: {seed.stderr}"

        # Fresh sqlite invocation, same VFS.
        read = bash.execute_sync("sqlite -header /tmp/notes.sqlite 'SELECT * FROM notes'")
        assert read.exit_code == 0, f"stderr was: {read.stderr}"
        assert "hello" in read.stdout


class TestDotCommands:
    @pytest.fixture
    def bash(self):
        return Bash(sqlite=True)

    def test_tables_command(self, bash):
        r = bash.execute_sync("sqlite :memory: 'CREATE TABLE one(a)' '\n.tables'")
        assert r.exit_code == 0, f"stderr was: {r.stderr}"
        assert r.stdout.strip() == "one"

    def test_dump_round_trip(self, bash):
        # `.dump` produces the schema + data SQL; feed it back via `.read`
        # and ensure the same query yields the same result.
        seed = bash.execute_sync("sqlite /tmp/src.sqlite 'CREATE TABLE t(x); INSERT INTO t VALUES (1), (2)'")
        assert seed.exit_code == 0
        dump = bash.execute_sync("sqlite /tmp/src.sqlite '.dump' > /tmp/dump.sql")
        assert dump.exit_code == 0
        result = bash.execute_sync("sqlite /tmp/dst.sqlite '.read /tmp/dump.sql' 'SELECT count(*) FROM t'")
        assert result.exit_code == 0, f"stderr was: {result.stderr}"
        assert result.stdout.strip() == "2"


class TestSecurityPolicy:
    """Policy hardening from PR #1507 must surface through the binding."""

    @pytest.fixture
    def bash(self):
        return Bash(sqlite=True)

    def test_attach_rejected(self, bash):
        r = bash.execute_sync("sqlite :memory: \"ATTACH DATABASE '/tmp/other.db' AS other\"")
        assert r.exit_code != 0
        assert "ATTACH/DETACH is not supported" in r.stderr

    def test_pragma_cache_size_blocked(self, bash):
        r = bash.execute_sync("sqlite :memory: 'PRAGMA cache_size = -1000'")
        assert r.exit_code != 0
        assert "PRAGMA cache_size is denied" in r.stderr

    def test_pragma_user_version_passes(self, bash):
        # Common operational PRAGMAs are not on the deny list.
        r = bash.execute_sync("sqlite :memory: 'PRAGMA user_version=42; PRAGMA user_version'")
        assert r.exit_code == 0, f"stderr was: {r.stderr}"
        assert "42" in r.stdout


class TestResetPreservesSqliteFlag:
    """`reset()` rebuilds the interpreter — the sqlite=True opt-in must
    survive so users don't get silently downgraded between runs."""

    def test_sqlite_survives_reset(self):
        bash = Bash(sqlite=True)
        # Sanity check before reset.
        before = bash.execute_sync("sqlite :memory: 'SELECT 1'")
        assert before.exit_code == 0, f"stderr was: {before.stderr}"

        bash.reset()

        after = bash.execute_sync("sqlite :memory: 'SELECT 2'")
        assert after.exit_code == 0, f"stderr was: {after.stderr}"
        assert after.stdout.strip() == "2"

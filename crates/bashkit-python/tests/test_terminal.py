"""bashkit.Terminal: interactive in-process terminal (vi, less, persistent shell)."""

import pytest

from bashkit import Bash, Terminal


def test_low_level_loop_runs_commands_and_reads_screen():
    t = Terminal(rows=10, cols=40)
    assert t.run_until_idle() == "idle"
    t.send("echo hello\r")
    assert t.run_until_idle() == "idle"
    assert t.screen_text() == "$ echo hello\nhello\n$"
    assert t.activity() == {"state": "prompt"}
    [record] = t.take_transcript()
    assert record == {
        "command": "echo hello",
        "output": "hello\n",
        "exit_code": 0,
        "output_truncated": False,
    }


def test_call_edits_file_in_vi():
    t = Terminal()
    out = t.call("vi /tmp/n.txt<Enter>")
    assert out["activity"] == "running"
    assert out["running_command"] == "vi /tmp/n.txt"
    assert out["full_screen"] is True
    out = t.call("ihello<Esc>:wq<Enter>")
    assert out["activity"] == "prompt"
    assert out["commands"][0]["exit_code"] == 0
    assert t.fs().read_file("/tmp/n.txt") == b"hello\n"


def test_history_keeps_scrolled_off_output():
    t = Terminal(rows=5, cols=30)
    t.call("seq 1 40<Enter>")
    assert "\n1\n2\n" in t.history_text()
    assert "\n1\n" not in t.screen_text()


def test_timeout_reports_running_command():
    t = Terminal()
    t.send("sleep 0.3; echo done\r")
    assert t.run_until_idle(timeout=0.02) == "timeout"
    assert t.activity()["state"] == "running"
    assert t.run_until_idle(timeout=5) == "idle"
    assert t.take_transcript()[0]["output"] == "done\n"


def test_wait_for_and_screen_changes():
    t = Terminal()
    out = t.call(
        "for i in 1 2 3; do echo tick$i; sleep 1; done<Enter>",
        wait_ms=10000,
        wait_for="tick2",
        screen="changes",
    )
    assert out["matched"] is True
    assert out["activity"] == "running"
    assert "screen" not in out
    assert any(c["text"] == "tick2" for c in out["screen_changes"])
    out = t.call(**{"input": "<C-c>", "screen": "changes"})
    assert out["activity"] == "prompt"
    assert [c["text"] for c in out["screen_changes"]][-2:] == ["^C", "$"]


def test_sessions_share_files():
    t = Terminal()
    t.call("echo hi > /tmp/s<Enter>")
    out = t.call("cat /tmp/s<Enter>", session="other")
    assert out["session"] == "other"
    assert out["commands"][0]["output"] == "hi\n"
    assert [s["name"] for s in out["sessions"]] == ["main", "other"]
    assert t.call(session="other", close=True)["closed"] is True


def test_exit_and_exit_code():
    t = Terminal()
    out = t.call("exit 3<Enter>")
    assert out["activity"] == "exited"
    assert out["exit_code"] == 3
    assert t.exit_code == 3
    assert t.run_until_idle() == "exited"


def test_options_apply_to_session():
    t = Terminal(username="ada", cwd="/tmp", env={"GREETING": "hi"})
    out = t.call("echo $GREETING $(whoami) $PWD<Enter>")
    assert out["commands"][0]["output"] == "hi ada /tmp\n"


def test_send_accepts_bytes_and_raw_output_is_drainable():
    t = Terminal()
    t.send(b"printf 'a\\nb\\n'\r")
    t.run_until_idle()
    assert b"a\r\nb\r\n" in t.take_output()
    assert t.take_output() == b""


def test_resize_and_size():
    t = Terminal(rows=24, cols=80)
    t.resize(30, 100)
    assert t.size() == (30, 100)
    out = t.call("echo $COLUMNS $LINES<Enter>")
    assert out["commands"][0]["output"] == "100 30\n"


def test_tool_metadata():
    t = Terminal()
    definition = t.tool_definition()
    assert definition["function"]["name"] == "terminal"
    assert "input" in definition["function"]["parameters"]["properties"]
    assert t.system_prompt().startswith("terminal:")


def test_invalid_arguments_raise():
    t = Terminal()
    with pytest.raises(ValueError):
        t.send(123)
    with pytest.raises(ValueError):
        t.run_until_idle(timeout=-1)
    with pytest.raises(ValueError):
        t.call("a" * (64 * 1024 + 1))


def test_pagers_stay_non_interactive_outside_terminal():
    bash = Bash()
    result = bash.execute_sync("seq 1 3 | less; vi /tmp/x")
    assert result.stdout == "1\n2\n3\n"
    assert result.exit_code == 1

"""Tests for ``Bash.analyze()`` / ``BashTool.analyze()``.

Static, pre-execution introspection used by host permission gates. Evasion
cases must surface as *unknown* (``is_opaque``), never as safe.
"""

import pytest

from bashkit import Bash, BashError, BashTool

READ_ONLY = {"cat", "grep", "ls", "echo", "wc", "head"}


def allowed(analysis) -> bool:
    """The documented host gate: allowlisted commands in a transparent script."""
    if analysis.is_opaque:
        return False
    return all(c.is_assignment_only or (c.name is not None and c.name in READ_ONLY) for c in analysis.commands)


class TestShape:
    def test_reports_commands_args_and_redirects(self):
        analysis = Bash().analyze("cat notes.txt | grep -i todo > out.txt")

        assert analysis.command_names == ["cat", "grep"]
        assert analysis.commands[0].name == "cat"
        assert analysis.commands[0].args == ["notes.txt"]
        assert analysis.commands[1].args == ["-i", "todo"]
        assert analysis.commands[0].context == "direct"
        assert len(analysis.redirects) == 1
        assert analysis.redirects[0].path == "out.txt"
        assert analysis.redirects[0].mode == "write"
        assert analysis.redirects[0].is_write is True
        assert analysis.is_opaque is False
        assert analysis.has_dynamic_commands is False
        assert analysis.has_interpreter_reentry is False
        assert analysis.truncated is False
        assert analysis.functions == []

    def test_redirect_modes(self):
        analysis = Bash().analyze("cat < in >> log 2>&1")
        assert len(analysis.redirects) == 2, "fd duplication names no file"
        assert analysis.redirects[0].mode == "read"
        assert analysis.redirects[0].is_write is False
        assert analysis.redirects[1].mode == "append"
        assert analysis.redirects[1].is_write is True

    def test_non_literal_words_are_none(self):
        analysis = Bash().analyze('rm "$target" /tmp/fixed /tmp/$name.txt')
        command = analysis.commands[0]

        assert command.name == "rm"
        assert command.args[0] is None
        assert command.args[1] == "/tmp/fixed"
        assert command.args[2] is None, "partial literals are not reconstructed"
        assert analysis.has_dynamic_commands is False

    def test_functions_and_contexts(self):
        analysis = Bash().analyze("cleanup() { rm -rf /data; }\necho $(date)")

        assert analysis.functions == ["cleanup"]
        assert analysis.commands[0].name == "rm"
        assert analysis.commands[0].context == "function_body"
        assert analysis.commands[1].context == "substitution"
        assert analysis.commands[2].context == "direct"
        assert analysis.has_command_substitution is True

    def test_assignment_only_command_is_not_dynamic(self):
        analysis = Bash().analyze("FOO=1")

        assert analysis.commands[0].name is None
        assert analysis.commands[0].assignments == ["FOO"]
        assert analysis.commands[0].is_assignment_only is True
        assert analysis.has_dynamic_commands is False
        assert analysis.is_opaque is False

    def test_prefix_assignments_ride_on_the_command(self):
        analysis = Bash().analyze("FOO=1 BAR=2 env")
        assert analysis.commands[0].name == "env"
        assert analysis.commands[0].assignments == ["FOO", "BAR"]
        assert analysis.commands[0].is_assignment_only is False

    def test_commands_named_filters(self):
        analysis = Bash().analyze("mydata record update 1; ls; mydata doc get 2")
        found = [c.args[0] for c in analysis.commands_named("mydata")]
        assert found == ["record", "doc"]
        assert analysis.commands_named("nope") == []

    def test_to_dict_and_repr(self):
        analysis = Bash().analyze("cat a > b")
        data = analysis.to_dict()

        assert data["command_names"] == ["cat"]
        assert data["commands"][0]["name"] == "cat"
        assert data["commands"][0]["args"] == ["a"]
        assert data["redirects"][0]["mode"] == "write"
        assert data["is_opaque"] is False
        assert "ScriptAnalysis(" in repr(analysis)
        assert "AnalyzedCommand(" in repr(analysis.commands[0])
        assert "AnalyzedRedirect(" in repr(analysis.redirects[0])


class TestEvasion:
    @pytest.mark.parametrize(
        "script",
        ["$cmd -rf /data", "c=rm; $c -rf /data", "$(echo rm) -rf /data"],
    )
    def test_dynamic_dispatch_is_opaque(self, script):
        analysis = Bash().analyze(script)
        assert analysis.has_dynamic_commands is True
        assert analysis.is_opaque is True
        assert not allowed(analysis)

    @pytest.mark.parametrize(
        "script",
        [
            'eval "$payload"',
            "source /tmp/x.sh",
            ". /tmp/x.sh",
            "bash -c 'rm -rf /data'",
            'sh -c "$payload"',
            "bash /tmp/setup.sh",
        ],
    )
    def test_interpreter_reentry_is_opaque(self, script):
        analysis = Bash().analyze(script)
        assert analysis.has_interpreter_reentry is True
        assert analysis.is_opaque is True

    @pytest.mark.parametrize(
        "script",
        [
            "echo $(rm -rf /data)",
            "echo `rm -rf /data`",
            "cat <(rm -rf /data)",
            "if true; then rm -rf /data; fi",
            "for f in a b; do rm $f; done",
            "case x in a) rm y ;; esac",
            "cleanup() { rm -rf /data; }; cleanup",
        ],
    )
    def test_hidden_commands_are_reported(self, script):
        analysis = Bash().analyze(script)
        assert "rm" in analysis.command_names
        assert not allowed(analysis)

    @pytest.mark.parametrize("script", ["if true; then", "echo 'unterminated"])
    def test_parse_errors_raise(self, script):
        with pytest.raises(BashError):
            Bash().analyze(script)

    def test_oversized_script_truncates_and_stays_opaque(self):
        analysis = Bash().analyze("echo x;" * 5000)
        assert analysis.truncated is True
        assert analysis.is_opaque is True
        assert not allowed(analysis)


class TestBehavior:
    def test_analyze_does_not_execute_or_mutate_state(self):
        bash = Bash()
        bash.analyze("mkdir -p /analyzed && echo marker > /analyzed/f")
        assert bash.execute_sync("test -e /analyzed; echo $?").stdout.strip() == "1"

        bash.analyze("FOO=bar")
        assert bash.execute_sync('echo "[$FOO]"').stdout.strip() == "[]"

    def test_analysis_matches_what_execution_dispatches(self):
        bash = Bash()
        script = "echo one > /tmp/a; cat /tmp/a | grep one"
        analysis = bash.analyze(script)

        assert analysis.command_names == ["echo", "cat", "grep"]
        assert [r.path for r in analysis.redirects] == ["/tmp/a"]
        result = bash.execute_sync(script)
        assert result.exit_code == 0
        assert result.stdout.strip() == "one"

    def test_allows_a_transparent_read_only_script(self):
        bash = Bash()
        assert allowed(bash.analyze("cat notes.txt | grep -i todo | wc -l"))
        assert allowed(bash.analyze("N=3; ls | head -n 3"))
        assert not allowed(bash.analyze("rm -rf /data"))

    def test_derives_fine_grained_permission_keys(self):
        bash = Bash()

        def key(script):
            commands = bash.analyze(script).commands_named("mydata")
            if not commands or len(commands[0].args) < 2:
                return None
            resource, action = commands[0].args[0], commands[0].args[1]
            if resource is None or action is None:
                return None
            if action in {"list", "get", "query"}:
                return None
            return f"mydata:{resource}:{action}"

        assert key("mydata record query 42") is None
        assert key("mydata record delete 42") == "mydata:record:delete"
        assert key("cat x | mydata doc write 7") == "mydata:doc:write"
        assert key("mydata record $action") is None

    def test_custom_builtins_are_visible_to_analysis(self):
        bash = Bash(custom_builtins={"mydata": lambda ctx: "ok\n"})
        analysis = bash.analyze("mydata record delete 1")
        assert analysis.command_names == ["mydata"]
        assert analysis.commands[0].args == ["record", "delete", "1"]

    def test_bashtool_analyze(self):
        tool = BashTool()
        assert tool.analyze("ls -la /tmp").command_names == ["ls"]
        with pytest.raises(BashError):
            tool.analyze("if true; then")

"""Exercise the real Deep Agents protocol against Bashkit's native VFS."""

import pytest

pytest.importorskip("deepagents")

from deepagents.backends.protocol import GrepResult, ReadResult, SandboxBackendProtocol  # noqa: E402

from bashkit.deepagents import BashkitBackend  # noqa: E402


@pytest.fixture
def backend():
    return BashkitBackend()


@pytest.mark.parametrize("redirect", ["", " >&2"])
def test_execute_reports_native_truncation(backend, redirect):
    response = backend.execute("printf '" + "x" * 1_100_000 + "'" + redirect)
    assert response.truncated is True
    assert len(response.output) == 1_048_576
    assert backend.execute("echo small").truncated is False


def test_current_protocol_file_operations(backend):
    assert isinstance(backend, SandboxBackendProtocol)
    assert backend.write("/tmp/file.txt", "one\ntwo\nthree").error is None
    assert backend.write("/tmp/file.txt", "overwrite").error is not None
    result = backend.read("/tmp/file.txt", offset=1, limit=1)
    assert isinstance(result, ReadResult)
    assert result.file_data == {"content": "two", "encoding": "utf-8"}
    assert (result.start_line, result.end_line, result.next_offset, result.total_lines) == (2, 2, 2, 3)
    assert backend.read("/tmp/file.txt", limit=0).no_lines_requested
    assert backend.read("/tmp/file.txt", offset=-1, limit=1).file_data["content"] == "one"
    assert backend.read("/missing").error is not None
    edit = backend.edit("/tmp/file.txt", "two", "changed")
    assert edit.error is None and edit.occurrences == 1
    assert backend.read("/tmp/file.txt").file_data["content"] == "one\nchanged\nthree"
    assert backend.ls("/tmp").entries == [{"path": "/tmp/file.txt", "is_dir": False, "size": 17}]
    assert backend.upload_files([("/tmp/upload.txt", b"exact")])[0].error is None
    assert backend.download_files(["/tmp/upload.txt"])[0].content == b"exact"
    assert backend.delete("/tmp/file.txt").error is None
    assert backend.delete("/tmp/file.txt").error is not None


@pytest.mark.parametrize(
    "glob,expected",
    [
        ("*.py", {"top.py", "src/test1.py", "src/deep/test2.py"}),
        ("src/**/*.py", {"src/test1.py", "src/deep/test2.py"}),
        ("/*.py", {"top.py"}),
        ("src/test[0-9].py", {"src/test1.py"}),
        ("*.absent", set()),
    ],
)
def test_grep_glob_and_literal_pattern(backend, glob, expected):
    backend.setup("mkdir -p /tmp/project/src/deep; cd /tmp/project")
    for path in ["top.py", "src/test1.py", "src/deep/test2.py", "src/test1.txt"]:
        assert backend.write(path, "a.b\naXb\n").error is None
    result = backend.grep("a.b", glob=glob)
    assert isinstance(result, GrepResult)
    assert result.error is None
    assert {m["path"].removeprefix("/tmp/project/") for m in result.matches} == expected
    assert all(m["line"] == 1 and m["text"] == "a.b" for m in result.matches)
    assert {m["path"] for m in backend.glob(glob).matches} == {m["path"] for m in result.matches}


def test_grep_handles_paths_patterns_and_limits(backend, tmp_path):
    host_only = tmp_path / "host-only.txt"
    host_only.write_text("secret")
    weird = "/tmp/a:b;$(touch injected).py"
    assert backend.write("/tmp/invalid\nname.py", "no").error
    assert backend.write(weird, "-needle\n-needle\n").error is None
    result = backend.grep("-needle", "/tmp", glob="*.py", max_count=1)
    assert result.matches == [{"path": weird, "line": 1, "text": "-needle"}]
    assert result.truncated
    assert not backend.grep("-needle", "/tmp", max_count=2).truncated
    assert backend.grep("-needle", "/tmp", max_count=0).truncated
    assert backend.grep("x", "/missing").error
    assert backend.grep("x", "/tmp", glob="../*").error
    assert backend.glob("../*", "/tmp").error
    assert backend.read(str(host_only)).error
    assert backend.execute("test -e injected").exit_code != 0
    backend.setup("ln -s /tmp /tmp/cycle")
    assert backend.grep("-needle", "/tmp", glob="*.py").matches == [
        {"path": weird, "line": number, "text": "-needle"} for number in (1, 2)
    ]


async def test_async_protocol_shares_middleware_vfs(backend):
    tool = backend.create_middleware().tools[0]
    assert "done" in tool.invoke({"command": "echo shared > /tmp/shared.txt; echo done"})
    assert (await backend.aread("/tmp/shared.txt")).file_data["content"] == "shared"
    assert (await backend.agrep("shared", "/tmp")).matches[0]["line"] == 1
    assert (await backend.aglob("*.txt", "/tmp")).matches[0]["path"] == "/tmp/shared.txt"
    assert (await backend.als("/tmp")).entries
    assert (await backend.aexecute("echo async")).output == "async\n"


def test_framework_filesystem_tools(backend):
    """Real agent middleware consumes the result contracts without an API call."""
    from deepagents import create_deep_agent
    from langchain_core.language_models.fake_chat_models import FakeMessagesListChatModel
    from langchain_core.messages import AIMessage

    class ScriptedModel(FakeMessagesListChatModel):
        def bind_tools(self, tools, **kwargs):
            return self

    calls = [
        ("write_file", {"file_path": "/tmp/protocol.py", "content": "needle\n"}),
        ("read_file", {"file_path": "/tmp/protocol.py"}),
        ("grep", {"pattern": "needle", "path": "/tmp", "glob": "*.py"}),
        ("glob", {"pattern": "*.py", "path": "/tmp"}),
        ("ls", {"path": "/tmp"}),
        ("execute", {"command": "echo framework-execute"}),
    ]
    model = ScriptedModel(
        responses=[
            AIMessage(content="", tool_calls=[{"name": name, "args": args, "id": str(i)}])
            for i, (name, args) in enumerate(calls)
        ]
        + [AIMessage(content="done")]
    )
    agent = create_deep_agent(model=model, backend=backend)
    response = agent.invoke({"messages": [{"role": "user", "content": "Exercise filesystem tools"}]})
    messages = [message for message in response["messages"] if message.type == "tool"]
    assert len(messages) == len(calls)
    assert all(message.status == "success" for message in messages)
    assert "needle" in str(messages[1].content)
    assert "/tmp/protocol.py" in str(messages[2].content)
    assert "framework-execute" in str(messages[-1].content)
    from deepagents.backends.protocol import execute_accepts_timeout

    assert not execute_accepts_timeout(type(backend))


def test_binary_transfers_and_large_exact_file(backend):
    content = bytes(range(256)) * 5000
    assert backend.upload_files([("/tmp/binary", content)])[0].error is None
    assert backend.download_files(["/tmp/binary"])[0].content == content
    assert backend.upload_files([("/tmp/binary", b"replacement")])[0].error is None
    assert backend.download_files(["/tmp/binary"])[0].content == b"replacement"
    assert backend.download_files(["/missing"])[0].error
    assert backend.upload_files([("/tmp", b"no")])[0].error
    assert backend.delete("/tmp").error
    text = "x" * 1_100_000
    assert backend.write("/tmp/large.txt", text).error is None
    assert backend.read("/tmp/large.txt").file_data["content"] == text
    assert backend.edit("/tmp/large.txt", text, "short").error is None
    assert backend.download_files(["/tmp/large.txt"])[0].content == b"short"

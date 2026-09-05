"""File API arguments must never become shell source."""

import pytest

pytest.importorskip("deepagents")

from bashkit.deepagents import BashkitBackend  # noqa: E402


@pytest.mark.parametrize(
    "path",
    [
        "/tmp/path with spaces/file.txt",
        "/tmp/a;touch INJECTED",
        "/tmp/$(touch INJECTED)",
        "/tmp/`touch INJECTED`",
        "/tmp/'quoted'",
        "/tmp/-option",
    ],
)
def test_file_operations_preserve_literal_paths_and_contents(path):
    backend = BashkitBackend()
    malicious = "BASHKIT_EOF\ntouch INJECTED\n$HOME\n`whoami`\n$(id)\n'single'\n\"double\"\n\\backslash"
    assert backend.write(path, malicious).error is None
    assert backend.download_files([path])[0].content == malicious.encode()
    assert backend.read(path).file_data["content"] == malicious
    assert backend.edit(path, malicious, "$(touch INJECTED)").error is None
    assert backend.download_files([path])[0].content == b"$(touch INJECTED)"
    assert backend.grep("$(touch INJECTED)", path).matches == [{"path": path, "line": 1, "text": "$(touch INJECTED)"}]
    assert backend.execute("test -e INJECTED").exit_code != 0

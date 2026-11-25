"""#3013: contain native-stack regressions in a timed child Python process."""

import subprocess
import sys
import textwrap

import pytest


@pytest.mark.parametrize(
    "filter_text",
    [
        "reduce range(100000) as $i (1; [.]) | length",
        "reduce range(100000) as $i (1; {a: .}) | empty",
        "try (reduce range(100000) as $i (1; [.])) catch 0",
        '("[" * 100000 + "0" + "]" * 100000) | fromjson | length',
    ],
)
def test_jq_generated_depth_does_not_abort_host(filter_text):
    code = textwrap.dedent("""
        import sys
        from bashkit import Bash

        bash = Bash()
        result = bash.execute_sync("jq -n '" + sys.argv[1] + "'")
        assert result.exit_code == 5, (result.exit_code, result.stderr)
        assert result.stderr == "jq: error: value nesting depth limit (100) exceeded\\n"
        assert result.stdout == ""
        recovered = bash.execute_sync("jq -n '1 + 1'")
        assert recovered.exit_code == 0
        assert recovered.stdout == "2\\n"
        assert recovered.stderr == ""
    """)
    # CI runs pytest from the package directory, where a bare `python -c`
    # child would import the source tree without `_bashkit`. Reuse the
    # parent's import paths so the child loads the same built extension.
    preamble = "import sys; sys.path[:0] = %r\n" % [p for p in sys.path if p]
    child = subprocess.run(
        [sys.executable, "-c", preamble + code, filter_text],
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert child.returncode == 0, f"child exit {child.returncode}: {child.stdout}{child.stderr}"

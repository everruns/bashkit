"""Tests for shell injection prevention in BashkitBackend (deepagents.py).

Verifies that user-supplied paths and patterns are properly quoted
with shlex.quote() to prevent command injection via f-string interpolation.

Ref: GitHub issue #411
"""

import shlex
from pathlib import Path

# Read deepagents.py source directly (BashkitBackend only exists when
# deepagents is installed, so we inspect source text instead).
_DEEPAGENTS_SRC = (Path(__file__).resolve().parent.parent / "bashkit" / "deepagents.py").read_text()


# -- Module-level checks -----------------------------------------------------


def test_shlex_imported_in_deepagents():
    """deepagents.py must import shlex for shell argument quoting."""
    assert "import shlex" in _DEEPAGENTS_SRC, "deepagents.py must import shlex"


def test_no_unquoted_cat_interpolation():
    """No raw f'cat {var}' patterns without shlex.quote."""
    # After fix, all cat uses should go through shlex.quote
    for line in _DEEPAGENTS_SRC.splitlines():
        stripped = line.strip()
        if stripped.startswith("#"):
            continue
        if 'f"cat {' in stripped or "f'cat {" in stripped:
            if "shlex.quote" not in stripped:
                assert False, f"Unquoted cat interpolation found: {stripped}"


def test_no_unquoted_ls_interpolation():
    """No raw f'ls -la {var}' patterns without shlex.quote."""
    for line in _DEEPAGENTS_SRC.splitlines():
        stripped = line.strip()
        if stripped.startswith("#"):
            continue
        if 'f"ls ' in stripped and "{" in stripped and "shlex.quote" not in stripped:
            assert False, f"Unquoted ls interpolation found: {stripped}"


def test_no_unquoted_find_interpolation():
    """No raw f'find {var}' patterns without shlex.quote."""
    for line in _DEEPAGENTS_SRC.splitlines():
        stripped = line.strip()
        if stripped.startswith("#"):
            continue
        if 'f"find {' in stripped or "f'find {" in stripped:
            if "shlex.quote" not in stripped:
                assert False, f"Unquoted find interpolation found: {stripped}"


def test_no_unquoted_grep_interpolation():
    """grep_raw must use shlex.quote for pattern and path."""
    # Extract grep_raw method body and verify shlex.quote is used
    in_grep_raw = False
    grep_raw_lines = []
    for line in _DEEPAGENTS_SRC.splitlines():
        if "def grep_raw(" in line:
            in_grep_raw = True
        elif in_grep_raw and (line.strip().startswith("def ") or line.strip().startswith("async def ")):
            break
        if in_grep_raw:
            grep_raw_lines.append(line)
    grep_raw_body = "\n".join(grep_raw_lines)
    assert "shlex.quote" in grep_raw_body, "grep_raw must use shlex.quote for pattern/path"


def test_shlex_quote_used_for_file_paths():
    """shlex.quote must appear in methods that interpolate file paths."""
    assert _DEEPAGENTS_SRC.count("shlex.quote") >= 7, (
        "Expected at least 7 uses of shlex.quote (read, write, edit, ls_info, glob_info, grep_raw, download_files)"
    )


# -- shlex.quote behavior validation -----------------------------------------


def test_shlex_quote_prevents_semicolon_injection():
    """shlex.quote must neutralize semicolon-based injection."""
    malicious = "/dev/null; echo pwned"
    quoted = shlex.quote(malicious)
    # Quoted string wraps in single quotes, preventing shell interpretation
    assert quoted.startswith("'"), "shlex.quote must single-quote dangerous input"
    assert ";" in quoted  # semicolon is inside quotes, not a command separator


def test_shlex_quote_prevents_backtick_injection():
    """shlex.quote must neutralize backtick-based injection."""
    malicious = "/tmp/`rm -rf /`/file"
    quoted = shlex.quote(malicious)
    assert quoted.startswith("'"), "shlex.quote must single-quote dangerous input"


def test_shlex_quote_prevents_dollar_expansion():
    """shlex.quote must neutralize $() command substitution."""
    malicious = "/tmp/$(cat /etc/passwd)/file"
    quoted = shlex.quote(malicious)
    assert quoted.startswith("'"), "shlex.quote must single-quote dangerous input"

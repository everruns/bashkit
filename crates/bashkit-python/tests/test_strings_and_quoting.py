"""String-handling and quoting-adjacent Python binding tests."""

# Decision: keep Python parity test names close to the Node suite so cross-
# binding coverage diffs stay mechanical.

import sys
from pathlib import Path

import pytest

from bashkit import Bash

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

_categories = __import__("_bashkit_categories")

_NAMES = ("test_bash_python_string_ops",)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
del _TESTS_DIR


def test_quoting_single_quotes_preserve_literal_value():
    bash = Bash()
    result = bash.execute_sync("echo '$HOME'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "$HOME"


def test_quoting_double_quotes_expand_variables():
    bash = Bash()
    bash.execute_sync("X=world")
    result = bash.execute_sync('echo "hello $X"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello world"


def test_quoting_double_quotes_preserve_spaces():
    bash = Bash()
    bash.execute_sync('X="hello   world"')
    result = bash.execute_sync('echo "$X"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello   world"


def test_quoting_backslash_escaping_in_double_quotes():
    bash = Bash()
    result = bash.execute_sync('echo "a\\$b"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "a$b"


def test_quoting_nested_command_substitution_in_quotes():
    bash = Bash()
    result = bash.execute_sync('echo "count: $(echo 42)"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "count: 42"


def test_quoting_heredoc_basic():
    bash = Bash()
    result = bash.execute_sync(
        """cat <<EOF
hello world
EOF"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello world"


def test_quoting_heredoc_with_variable_expansion():
    bash = Bash()
    bash.execute_sync("NAME=alice")
    result = bash.execute_sync(
        """cat <<EOF
hello $NAME
EOF"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello alice"


def test_quoting_heredoc_quoted_delimiter_suppresses_expansion():
    bash = Bash()
    bash.execute_sync("NAME=alice")
    result = bash.execute_sync(
        """cat <<'EOF'
hello $NAME
EOF"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello $NAME"


def test_quoting_string_concatenation():
    bash = Bash()
    bash.execute_sync("A=hello; B=world")
    result = bash.execute_sync('echo "${A}${B}"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "helloworld"


def test_quoting_string_replacement_first_match():
    bash = Bash()
    bash.execute_sync("S='hello world hello'")
    result = bash.execute_sync('echo "${S/hello/bye}"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "bye world hello"


def test_quoting_string_replacement_global():
    bash = Bash()
    bash.execute_sync("S='hello world hello'")
    result = bash.execute_sync('echo "${S//hello/bye}"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "bye world bye"


def test_quoting_uppercase_conversion():
    bash = Bash()
    bash.execute_sync("S=hello")
    result = bash.execute_sync('echo "${S^^}"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "HELLO"


def test_quoting_lowercase_conversion():
    bash = Bash()
    bash.execute_sync("S=HELLO")
    result = bash.execute_sync('echo "${S,,}"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello"


def test_quoting_array_declaration_and_access():
    bash = Bash()
    bash.execute_sync("ARR=(apple banana cherry)")
    first = bash.execute_sync('echo "${ARR[0]}"')
    last = bash.execute_sync('echo "${ARR[2]}"')
    assert first.exit_code == 0
    assert first.stdout.strip() == "apple"
    assert last.exit_code == 0
    assert last.stdout.strip() == "cherry"


def test_quoting_array_length():
    bash = Bash()
    bash.execute_sync("ARR=(a b c d)")
    result = bash.execute_sync('echo "${#ARR[@]}"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "4"


def test_quoting_array_all_elements():
    bash = Bash()
    bash.execute_sync("ARR=(x y z)")
    result = bash.execute_sync('echo "${ARR[@]}"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "x y z"


def test_quoting_array_append():
    bash = Bash()
    bash.execute_sync("ARR=(a b)")
    bash.execute_sync("ARR+=(c)")
    result = bash.execute_sync('echo "${ARR[@]}"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "a b c"


def test_quoting_array_iteration_in_for_loop():
    bash = Bash()
    result = bash.execute_sync(
        """
FRUITS=(apple banana cherry)
for fruit in "${FRUITS[@]}"; do
  echo "$fruit"
done
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "apple\nbanana\ncherry"


def test_quoting_empty_string_variable():
    bash = Bash()
    bash.execute_sync('X=""')
    result = bash.execute_sync('echo "[$X]"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "[]"


def test_quoting_newlines_in_variable():
    bash = Bash()
    bash.execute_sync('X="line1\nline2"')
    result = bash.execute_sync('echo -e "$X"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "line1\nline2"


def test_quoting_tab_character_preserved():
    bash = Bash()
    result = bash.execute_sync("printf 'a\\tb'")
    assert result.exit_code == 0
    assert result.stdout == "a\tb"


def test_quoting_semicolon_separates_commands():
    bash = Bash()
    result = bash.execute_sync("echo a; echo b")
    assert result.exit_code == 0
    assert result.stdout.strip() == "a\nb"


def test_quoting_long_string_handling():
    bash = Bash()
    long_value = "x" * 10000
    result = bash.execute_sync(f"X={long_value}; echo ${{#X}}")
    assert result.exit_code == 0
    assert result.stdout.strip() == "10000"


@pytest.mark.asyncio
async def test_quoting_async_double_quotes_expand_variables():
    bash = Bash()
    bash.execute_sync("X=world")
    result = await bash.execute('echo "hello $X"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello world"


@pytest.mark.asyncio
async def test_quoting_async_array_append():
    bash = Bash()
    bash.execute_sync("ARR=(a b)")
    result = await bash.execute('ARR+=(c); echo "${ARR[@]}"')
    assert result.exit_code == 0
    assert result.stdout.strip() == "a b c"

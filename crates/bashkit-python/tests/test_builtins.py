"""Builtin-command coverage pulled out of the legacy Python test module."""

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

_NAMES = ("test_bash_pipeline",)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
del _TESTS_DIR


def test_builtin_cat_reads_file():
    bash = Bash()
    bash.execute_sync("printf 'hello\\n' > /tmp/cat.txt")
    result = bash.execute_sync("cat /tmp/cat.txt")
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello"


def test_builtin_cat_concatenates_files():
    bash = Bash()
    bash.execute_sync("printf 'a\\n' > /tmp/a.txt")
    bash.execute_sync("printf 'b\\n' > /tmp/b.txt")
    result = bash.execute_sync("cat /tmp/a.txt /tmp/b.txt")
    assert result.exit_code == 0
    assert result.stdout.strip() == "a\nb"


def test_builtin_head_limits_lines():
    bash = Bash()
    bash.execute_sync("printf '1\\n2\\n3\\n4\\n5\\n' > /tmp/head.txt")
    result = bash.execute_sync("head -n 2 /tmp/head.txt")
    assert result.exit_code == 0
    assert result.stdout.strip() == "1\n2"


def test_builtin_tail_limits_lines():
    bash = Bash()
    bash.execute_sync("printf '1\\n2\\n3\\n4\\n5\\n' > /tmp/tail.txt")
    result = bash.execute_sync("tail -n 2 /tmp/tail.txt")
    assert result.exit_code == 0
    assert result.stdout.strip() == "4\n5"


def test_builtin_wc_counts_lines():
    bash = Bash()
    bash.execute_sync("printf 'a\\nb\\nc\\n' > /tmp/wc.txt")
    result = bash.execute_sync("wc -l < /tmp/wc.txt")
    assert result.exit_code == 0
    assert result.stdout.strip() == "3"


def test_builtin_wc_counts_words():
    bash = Bash()
    result = bash.execute_sync("printf 'one two three\\n' | wc -w")
    assert result.exit_code == 0
    assert result.stdout.strip() == "3"


def test_builtin_grep_basic_match():
    bash = Bash()
    result = bash.execute_sync("printf 'apple\\nbanana\\ncherry\\n' | grep banana")
    assert result.exit_code == 0
    assert result.stdout.strip() == "banana"


def test_builtin_grep_case_insensitive():
    bash = Bash()
    result = bash.execute_sync("printf 'Hello\\nworld\\n' | grep -i hello")
    assert result.exit_code == 0
    assert result.stdout.strip() == "Hello"


def test_builtin_grep_inverted_match():
    bash = Bash()
    result = bash.execute_sync("printf 'a\\nb\\nc\\n' | grep -v b")
    assert result.exit_code == 0
    assert result.stdout.strip() == "a\nc"


def test_builtin_grep_counts_matches():
    bash = Bash()
    result = bash.execute_sync("printf 'aa\\nab\\nac\\nbb\\n' | grep -c '^a'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "3"


def test_builtin_grep_no_match_returns_non_zero():
    bash = Bash()
    result = bash.execute_sync("printf 'hello\\n' | grep xyz")
    assert result.exit_code != 0
    assert result.stdout == ""


def test_builtin_sed_substitutes_first_match():
    bash = Bash()
    result = bash.execute_sync("printf 'hello world\\n' | sed 's/world/earth/'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello earth"


def test_builtin_sed_substitutes_globally():
    bash = Bash()
    result = bash.execute_sync("printf 'aaa\\n' | sed 's/a/b/g'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "bbb"


def test_builtin_sed_deletes_matching_line():
    bash = Bash()
    result = bash.execute_sync("printf 'a\\nb\\nc\\n' | sed '/b/d'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "a\nc"


def test_builtin_awk_prints_selected_field():
    bash = Bash()
    result = bash.execute_sync("printf 'one two three\\n' | awk '{print $2}'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "two"


def test_builtin_awk_honors_custom_separator():
    bash = Bash()
    result = bash.execute_sync("printf 'a:b:c\\n' | awk -F: '{print $3}'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "c"


def test_builtin_awk_sums_numeric_column():
    bash = Bash()
    result = bash.execute_sync("printf '1\\n2\\n3\\n4\\n' | awk '{s+=$1} END {print s}'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "10"


def test_builtin_sort_ascending():
    bash = Bash()
    result = bash.execute_sync("printf 'c\\na\\nb\\n' | sort")
    assert result.exit_code == 0
    assert result.stdout.strip() == "a\nb\nc"


def test_builtin_sort_descending():
    bash = Bash()
    result = bash.execute_sync("printf 'a\\nb\\nc\\n' | sort -r")
    assert result.exit_code == 0
    assert result.stdout.strip() == "c\nb\na"


def test_builtin_sort_numeric():
    bash = Bash()
    result = bash.execute_sync("printf '10\\n2\\n1\\n20\\n' | sort -n")
    assert result.exit_code == 0
    assert result.stdout.strip() == "1\n2\n10\n20"


def test_builtin_uniq_removes_adjacent_duplicates():
    bash = Bash()
    result = bash.execute_sync("printf 'a\\na\\nb\\nb\\nc\\n' | uniq")
    assert result.exit_code == 0
    assert result.stdout.strip() == "a\nb\nc"


def test_builtin_sort_and_uniq_count_duplicates():
    bash = Bash()
    result = bash.execute_sync("printf 'a\\nb\\na\\na\\n' | sort | uniq -c")
    assert result.exit_code == 0
    assert "3" in result.stdout
    assert "a" in result.stdout


def test_builtin_tr_transliterates_characters():
    bash = Bash()
    result = bash.execute_sync("printf 'hello\\n' | tr 'a-z' 'A-Z'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "HELLO"


def test_builtin_tr_deletes_characters():
    bash = Bash()
    result = bash.execute_sync("printf 'h-e-l-l-o\\n' | tr -d '-'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "hello"


def test_builtin_cut_extracts_requested_field():
    bash = Bash()
    result = bash.execute_sync("printf 'a,b,c\\n' | cut -d, -f2")
    assert result.exit_code == 0
    assert result.stdout.strip() == "b"


def test_builtin_printf_formats_basic_string():
    bash = Bash()
    result = bash.execute_sync('printf "Hello %s" "World"')
    assert result.exit_code == 0
    assert result.stdout == "Hello World"


def test_builtin_printf_formats_numbers():
    bash = Bash()
    result = bash.execute_sync('printf "%d + %d = %d" 2 3 5')
    assert result.exit_code == 0
    assert result.stdout == "2 + 3 = 5"


def test_builtin_export_and_env_expose_variable():
    bash = Bash()
    bash.execute_sync("export MY_VAR=hello")
    result = bash.execute_sync("env | grep '^MY_VAR='")
    assert result.exit_code == 0
    assert result.stdout.strip() == "MY_VAR=hello"


def test_builtin_unset_clears_variable():
    bash = Bash()
    bash.execute_sync("X=123")
    bash.execute_sync("unset X")
    result = bash.execute_sync("echo ${X:-gone}")
    assert result.exit_code == 0
    assert result.stdout.strip() == "gone"


def test_builtin_jq_extracts_field():
    bash = Bash()
    result = bash.execute_sync('printf \'{"name":"alice","age":30}\\n\' | jq -r \'.name\'')
    assert result.exit_code == 0
    assert result.stdout.strip() == "alice"


def test_builtin_jq_reports_array_length():
    bash = Bash()
    result = bash.execute_sync("printf '[1,2,3]\\n' | jq 'length'")
    assert result.exit_code == 0
    assert result.stdout.strip() == "3"


def test_builtin_jq_filters_array_rows():
    bash = Bash()
    result = bash.execute_sync(
        'printf \'[{"name":"alice","age":30},{"name":"bob","age":25}]\\n\' | jq -r \'.[] | select(.age > 28) | .name\''
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "alice"


def test_builtin_md5sum_produces_expected_hash():
    bash = Bash()
    result = bash.execute_sync("printf 'hello' | md5sum")
    assert result.exit_code == 0
    assert result.stdout.startswith("5d41402abc4b2a76b9719d911017c592")


def test_builtin_sha256sum_produces_expected_hash():
    bash = Bash()
    result = bash.execute_sync("printf 'hello' | sha256sum")
    assert result.exit_code == 0
    assert result.stdout.startswith("2cf24dba5fb0a30e26e83b2ac5b9e29e")


def test_builtin_date_runs_without_error():
    bash = Bash()
    result = bash.execute_sync("date")
    assert result.exit_code == 0
    assert result.stdout.strip()


def test_builtin_base64_encodes_and_decodes():
    bash = Bash()
    encoded = bash.execute_sync("printf 'hello' | base64")
    assert encoded.exit_code == 0
    assert encoded.stdout.strip() == "aGVsbG8="

    decoded = bash.execute_sync("printf '%s' 'aGVsbG8=' | base64 -d")
    assert decoded.exit_code == 0
    assert decoded.stdout == "hello"


def test_builtin_seq_generates_range():
    bash = Bash()
    result = bash.execute_sync("seq 3 5")
    assert result.exit_code == 0
    assert result.stdout.strip() == "3\n4\n5"


def test_builtin_seq_generates_range_with_step():
    bash = Bash()
    result = bash.execute_sync("seq 1 2 5")
    assert result.exit_code == 0
    assert result.stdout.strip() == "1\n3\n5"


@pytest.mark.asyncio
async def test_builtin_async_grep_case_insensitive():
    bash = Bash()
    result = await bash.execute("printf 'Hello\\nworld\\n' | grep -i hello")
    assert result.exit_code == 0
    assert result.stdout.strip() == "Hello"


@pytest.mark.asyncio
async def test_builtin_async_jq_filters_array_rows():
    bash = Bash()
    result = await bash.execute(
        'printf \'[{"name":"alice","age":30},{"name":"bob","age":25}]\\n\' | jq -r \'.[] | select(.age > 28) | .name\''
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "alice"

"""Script-shaped workflows and higher-level Python binding scenarios."""

# Decision: keep the legacy high-level coverage while adding Node-parity script
# scenarios here so the file becomes the canonical script-pattern suite.

import json
import sys
from pathlib import Path

import pytest

from bashkit import Bash, BashTool

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

_categories = __import__("_bashkit_categories")

_NAMES = (
    "test_bash_file_persistence",
    "test_file_persistence",
    "test_multiline",
    "test_scripted_tool_pipeline_with_jq",
    "test_scripted_tool_multi_step",
    "test_scripted_tool_error_fallback",
    "test_scripted_tool_stdin_pipe",
    "test_scripted_tool_env_var",
    "test_scripted_tool_boolean_flag",
    "test_scripted_tool_integer_coercion",
    "test_async_multiple_tools",
    "test_scripted_tool_dozen_tools",
    "test_bash_external_handler_called",
    "test_bash_external_handler_multiple_calls",
    "test_bash_external_handler_returns_dict",
    "test_bash_external_handler_vfs_and_tools_together",
    "test_bash_reset_preserves_python_and_handler",
    "test_bash_external_handler_without_external_functions",
    "test_bash_bigint_roundtrip",
    "test_bash_bigint_roundtrip_value",
    "test_bash_async_callable_object_accepted",
    "test_bash_handler_receives_set_as_set",
)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
del _TESTS_DIR


def test_script_count_lines_in_file():
    bash = Bash()
    bash.execute_sync("printf 'a\\nb\\nc\\nd\\ne\\n' > /tmp/lines.txt")
    result = bash.execute_sync("wc -l < /tmp/lines.txt")
    assert result.exit_code == 0
    assert result.stdout.strip() == "5"


def test_script_find_and_replace_in_file():
    bash = Bash()
    bash.execute_sync("printf 'Hello World\\n' > /tmp/replace.txt")
    bash.execute_sync("sed -i 's/World/Earth/' /tmp/replace.txt")
    result = bash.execute_sync("cat /tmp/replace.txt")
    assert result.exit_code == 0
    assert result.stdout.strip() == "Hello Earth"


def test_script_extract_unique_values():
    bash = Bash()
    result = bash.execute_sync("printf 'apple\\nbanana\\napple\\ncherry\\nbanana\\n' | sort -u")
    assert result.exit_code == 0
    assert result.stdout.strip() == "apple\nbanana\ncherry"


def test_script_json_processing_pipeline():
    bash = Bash()
    result = bash.execute_sync(
        """
printf '[{"name":"alice","age":30},{"name":"bob","age":25}]\\n' | \
  jq -r '.[] | select(.age > 28) | .name'
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "alice"


def test_script_create_directory_tree_and_verify():
    bash = Bash()
    bash.execute_sync("mkdir -p /tmp/project/{src,lib,test}")
    bash.execute_sync("touch /tmp/project/src/main.sh")
    bash.execute_sync("touch /tmp/project/test/test.sh")
    result = bash.execute_sync("ls /tmp/project/src")
    assert result.exit_code == 0
    assert result.stdout.strip() == "main.sh"


def test_script_config_file_generator():
    bash = Bash()
    result = bash.execute_sync(
        """
APP_NAME=myapp
APP_PORT=8080
cat <<EOF
{
  "name": "$APP_NAME",
  "port": $APP_PORT
}
EOF
"""
    )
    assert result.exit_code == 0
    config = json.loads(result.stdout)
    assert config == {"name": "myapp", "port": 8080}


def test_script_loop_with_accumulator():
    bash = Bash()
    result = bash.execute_sync(
        """
SUM=0
for n in 1 2 3 4 5; do
  SUM=$((SUM + n))
done
echo $SUM
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "15"


def test_script_data_transformation_pipeline():
    bash = Bash()
    bash.execute_sync("printf 'Alice,30\\nBob,25\\nCharlie,35\\n' > /tmp/data.csv")
    result = bash.execute_sync("cat /tmp/data.csv | sort -t, -k2 -n | head -1 | cut -d, -f1")
    assert result.exit_code == 0
    assert result.stdout.strip() == "Bob"


def test_script_error_handling_with_or_fallback():
    bash = Bash()
    result = bash.execute_sync("cat /nonexistent/file 2>/dev/null || echo fallback")
    assert result.exit_code == 0
    assert result.stdout.strip() == "fallback"


def test_script_conditional_file_creation():
    bash = Bash()
    result = bash.execute_sync(
        """
FILE=/tmp/conditional.txt
if [ ! -f "$FILE" ]; then
  echo created > "$FILE"
fi
cat "$FILE"
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "created"


def test_script_function_with_multiple_operations():
    bash = Bash()
    result = bash.execute_sync(
        """
process_list() {
  local items="$1"
  echo "$items" | tr ',' '\n' | sort | while read item; do
    echo "- $item"
  done
}
process_list "cherry,apple,banana"
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "- apple\n- banana\n- cherry"


def test_script_nested_loops_multiline():
    bash = Bash()
    result = bash.execute_sync(
        """
for i in 1 2; do
  for j in a b; do
    echo -n "$i$j "
  done
done
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "1a 1b 2a 2b"


def test_script_while_read_loop_multiline():
    bash = Bash()
    result = bash.execute_sync(
        """
printf '1:one\n2:two\n3:three\n' | while IFS=: read num word; do
  echo "$word=$num"
done
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "one=1\ntwo=2\nthree=3"


def test_script_bashtool_llm_style_single_command():
    tool = BashTool()
    result = tool.execute_sync("echo 'Hello from the AI agent'")
    assert result.exit_code == 0
    assert "Hello from the AI agent" in result.stdout


def test_script_bashtool_llm_style_multi_step_script():
    tool = BashTool()
    result = tool.execute_sync(
        """
mkdir -p /tmp/workspace
echo '{"status": "ok"}' > /tmp/workspace/result.json
cat /tmp/workspace/result.json | jq -r '.status'
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "ok"


def test_script_bashtool_llm_style_data_analysis():
    tool = BashTool()
    tool.execute_sync("printf '2024-01-01,100\\n2024-01-02,200\\n2024-01-03,150\\n' > /tmp/sales.csv")
    total = tool.execute_sync("awk -F, '{sum+=$2} END {print sum}' /tmp/sales.csv")
    count = tool.execute_sync("wc -l < /tmp/sales.csv")
    assert total.exit_code == 0
    assert total.stdout.strip() == "450"
    assert count.exit_code == 0
    assert count.stdout.strip() == "3"


def test_script_bashtool_sequential_calls_build_state():
    tool = BashTool()
    tool.execute_sync("mkdir -p /tmp/project && cd /tmp/project")
    tool.execute_sync("printf '# My Project\\n' > /tmp/project/README.md")
    tool.execute_sync("printf 'fn main() {}\\n' > /tmp/project/main.rs")
    result = tool.execute_sync("ls /tmp/project")
    assert result.exit_code == 0
    assert "README.md" in result.stdout
    assert "main.rs" in result.stdout


def test_script_many_sequential_commands():
    bash = Bash()
    for i in range(50):
        result = bash.execute_sync(f"echo {i}")
        assert result.exit_code == 0
    final = bash.execute_sync("echo done")
    assert final.exit_code == 0
    assert final.stdout.strip() == "done"


def test_script_large_output():
    bash = Bash()
    result = bash.execute_sync("seq 1 1000")
    lines = result.stdout.strip().splitlines()
    assert result.exit_code == 0
    assert len(lines) == 1000
    assert lines[0] == "1"
    assert lines[-1] == "1000"


def test_script_empty_stdin_pipe():
    bash = Bash()
    result = bash.execute_sync("printf '' | grep 'x'")
    assert result.exit_code != 0
    assert result.stdout == ""


@pytest.mark.asyncio
async def test_script_async_json_processing_pipeline():
    bash = Bash()
    result = await bash.execute(
        """
printf '[{"name":"alice","age":30},{"name":"bob","age":25}]\\n' | \
  jq -r '.[] | select(.age > 28) | .name'
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "alice"


@pytest.mark.asyncio
async def test_script_async_bashtool_llm_style_multi_step_script():
    tool = BashTool()
    result = await tool.execute(
        """
mkdir -p /tmp/workspace
echo '{"status": "ok"}' > /tmp/workspace/result.json
cat /tmp/workspace/result.json | jq -r '.status'
"""
    )
    assert result.exit_code == 0
    assert result.stdout.strip() == "ok"

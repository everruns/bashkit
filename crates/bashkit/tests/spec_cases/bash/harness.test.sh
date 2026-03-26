### harness_exec_builtin_runs_script
# exec with command runs the script and terminates
echo '#!/bin/bash
echo "from-script"' > /tmp/cmd.sh
chmod +x /tmp/cmd.sh
exec /tmp/cmd.sh
echo "should-not-print"
### expect
from-script
### end

### harness_exec_builtin_with_args
# exec passes args to the command
echo '#!/bin/bash
echo "got: $1 $2"' > /tmp/ea.sh
chmod +x /tmp/ea.sh
exec /tmp/ea.sh hello world
### expect
got: hello world
### end

### harness_exec_builtin_exit_code
# exec propagates exit code
echo '#!/bin/bash
exit 42' > /tmp/erc.sh
chmod +x /tmp/erc.sh
exec /tmp/erc.sh
### expect
### end

### harness_exec_builtin_builtin_cmd
# exec with a builtin command
exec echo "exec-echo"
echo "should-not-print"
### expect
exec-echo
### end

### harness_bash_source_script
# BASH_SOURCE is set when executing a script
echo '#!/bin/bash
echo "${BASH_SOURCE[0]}"' > /tmp/bs.sh
chmod +x /tmp/bs.sh
/tmp/bs.sh
### expect
/tmp/bs.sh
### end

### harness_bash_source_equals_dollar_zero
# BASH_SOURCE[0] == $0 when script is executed directly
echo '#!/bin/bash
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  echo "match"
else
  echo "no-match"
fi' > /tmp/bsz.sh
chmod +x /tmp/bsz.sh
/tmp/bsz.sh
### expect
match
### end

### harness_bash_source_sourced_file
# BASH_SOURCE[0] is the sourced file when using source
echo 'echo "${BASH_SOURCE[0]}"' > /tmp/src.sh
source /tmp/src.sh
### expect
/tmp/src.sh
### end

### harness_declare_A_nameref
# declare -A with local -n reference (harness pattern)
declare -A hook_map
hook_map["10-init"]="/plugins/core/hooks.d/start/10-init"
hook_map["20-tools"]="/plugins/core/hooks.d/assemble/20-tools"

_collect_hooks() {
  local -n map_ref="$1"
  map_ref["30-prompts"]="/plugins/core/hooks.d/assemble/30-prompts"
}

_collect_hooks hook_map
echo "${hook_map[30-prompts]}"
echo "${#hook_map[@]}"
### expect
/plugins/core/hooks.d/assemble/30-prompts
3
### end

### harness_ifs_colon_read_ra
# IFS=':' read -ra splits colon-separated list (harness source parsing)
HARNESS_SOURCES="/a/b:/c/d:/e/f"
IFS=':' read -ra sources <<< "${HARNESS_SOURCES}"
echo "${#sources[@]}"
echo "${sources[0]}"
echo "${sources[2]}"
### expect
3
/a/b
/e/f
### end

### harness_c_style_for_reverse
# C-style reverse for loop (harness priority resolution)
arr=("one" "two" "three")
for (( i=${#arr[@]}-1; i>=0; i-- )); do
  echo "${arr[i]}"
done
### expect
three
two
one
### end

### harness_assoc_array_iteration
# Iterate assoc array keys (harness tool/hook discovery)
declare -A tools
tools["bash"]="/bin/bash"
tools["read_file"]="/usr/bin/read_file"
tools["write_file"]="/usr/bin/write_file"
for name in "${!tools[@]}"; do
  echo "${name}: ${tools[$name]}"
done | sort
### expect
bash: /bin/bash
read_file: /usr/bin/read_file
write_file: /usr/bin/write_file
### end

### harness_readlink_f
# readlink -f canonicalizes path (harness root resolution)
mkdir -p /home/user/project/bin
readlink -f /home/user/project/bin/../bin
### expect
/home/user/project/bin
### end

### harness_date_iseconds
# date -Iseconds outputs ISO 8601 with seconds
result="$(date -Iseconds)"
# Just check it contains a T separator and a timezone offset
[[ "${result}" == *T* ]] && echo "has-T"
### expect
has-T
### end

### harness_mapfile_sort
# mapfile -t reads sorted lines (harness hook ordering)
echo -e "20-tools\n10-init\n30-prompts" | sort > /tmp/hooks.txt
mapfile -t sorted < /tmp/hooks.txt
echo "${sorted[0]}"
echo "${sorted[1]}"
echo "${sorted[2]}"
### expect
10-init
20-tools
30-prompts
### end

### harness_readonly_var
# readonly prevents reassignment
HARNESS_VERSION="0.1.0"
readonly HARNESS_VERSION
echo "${HARNESS_VERSION}"
### expect
0.1.0
### end

### harness_set_euo_pipefail
# set -euo pipefail works together
set -euo pipefail
x="hello"
echo "${x}"
### expect
hello
### end

### harness_heredoc_with_vars
# Here document with variable expansion (harness session.md)
id="20260324-143022"
model="claude-sonnet-4-20250514"
cat <<EOF
---
id: ${id}
model: ${model}
---
EOF
### expect
---
id: 20260324-143022
model: claude-sonnet-4-20250514
---
### end

### harness_printf_04d
# printf %04d for sequence numbers (harness session messages)
printf '%04d' 1
echo
printf '%04d' 42
echo
### expect
0001
0042
### end

### harness_regex_match_capture
# [[ =~ ]] with BASH_REMATCH capture groups (harness message parsing)
line='```tool_call id=abc123 name=bash'
if [[ "${line}" =~ ^\`\`\`tool_call\ id=([^ ]+)\ name=([^ ]+) ]]; then
  echo "id: ${BASH_REMATCH[1]}"
  echo "name: ${BASH_REMATCH[2]}"
fi
### expect
id: abc123
name: bash
### end

### harness_case_esac_pattern
# case/esac with various patterns (harness CLI parsing)
for arg in "--help" "-h" "run" "unknown"; do
  case "${arg}" in
    --help|-h) echo "${arg}: help" ;;
    run) echo "${arg}: run" ;;
    *) echo "${arg}: other" ;;
  esac
done
### expect
--help: help
-h: help
run: run
unknown: other
### end

### harness_source_and_call_functions
# Source a file and call its functions (harness command pattern)
cat > /tmp/lib.sh << 'EOF'
session_new() {
  echo "new-session-123"
}
_log() {
  echo "[log] $*" >&2
}
EOF
source /tmp/lib.sh
result="$(session_new)"
echo "${result}"
### expect
new-session-123
### end

### harness_script_source_guard
# The BASH_SOURCE[0] == $0 guard pattern (harness main guard)
cat > /tmp/guarded.sh << 'SCRIPT'
#!/bin/bash
main() {
  echo "main ran"
}
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
SCRIPT
chmod +x /tmp/guarded.sh
# Execute directly — should run main
/tmp/guarded.sh
### expect
main ran
### end

### harness_script_source_guard_sourced
# When sourced, main should NOT run
cat > /tmp/guarded2.sh << 'SCRIPT'
main() {
  echo "main ran"
}
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
SCRIPT
source /tmp/guarded2.sh
echo "source done"
main
### expect
source done
main ran
### end

### harness_pipeline_hooks
# Simulated hook pipeline: output of one feeds next (harness run_hooks pattern)
current='{"stage":"start"}'
# Hook 1: adds field via jq --arg
current="$(echo "${current}" | jq --arg v init '. + {key: $v}')"
echo "${current}" | jq -r '.stage'
echo "${current}" | jq -r '.key'
### expect
start
init
### end

### harness_jq_argjson_pipeline
# jq --argjson for tool schema building (harness tool discovery)
schemas='[]'
schema='{"name":"bash"}'
schemas="$(echo "${schemas}" | jq --argjson s "${schema}" '. + [$s]')"
echo "${schemas}" | jq -r '.[0].name'
### expect
bash
### end

### harness_mktemp_and_trap
# mktemp with trap cleanup (harness send hook pattern)
tmp="$(mktemp)"
trap 'rm -f "${tmp}"' EXIT
echo "created" > "${tmp}"
cat "${tmp}"
### expect
created
### end

### harness_command_v_check
# command -v for dependency checking (harness _require pattern)
command -v jq &>/dev/null && echo "jq found"
command -v nonexistent_cmd &>/dev/null || echo "not found"
### expect
jq found
not found
### end

### harness_walk_dirs_upward
# Walk directory tree upward (harness source discovery pattern)
mkdir -p /home/user/project/sub
dirs=()
cur="/home/user/project/sub"
while true; do
  dirs+=("${cur}")
  [[ "${cur}" == "/" ]] && break
  cur="$(dirname "${cur}")"
done
echo "${#dirs[@]}"
echo "${dirs[0]}"
echo "${dirs[${#dirs[@]}-1]}"
### expect
5
/home/user/project/sub
/
### end

### harness_default_next_assoc
# Associative array state machine (harness DEFAULT_NEXT)
declare -A DEFAULT_NEXT=(
  [start]=assemble
  [assemble]=send
  [send]=receive
  [receive]=done
  [tool_exec]=tool_done
  [tool_done]=assemble
  [error]=done
)
state="start"
echo "${DEFAULT_NEXT[${state}]}"
state="${DEFAULT_NEXT[${state}]}"
echo "${state}"
echo "${DEFAULT_NEXT[${state}]}"
### expect
assemble
assemble
send
### end

### harness_seq_printf_loop
### bash_diff: ${#var} counts chars in bashkit but bytes in real bash (no LANG/LC_ALL set)
# Combined seq + printf tool display pattern
tool_name="bash"
len=${#tool_name}
remaining=$((50 - len))
line="── ${tool_name} "
for i in $(seq 1 ${remaining}); do
  line+="─"
done
echo "${#line}" # 3 (── ) + 4 (bash) + 1 ( ) + 46 (─'s) = 54 characters
### expect
54
### end

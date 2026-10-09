### errexit_negated_pipeline_does_not_exit
set -e
! true; echo "after !"
! false; echo "after ! false"
### expect
after !
after ! false
### end

### errexit_ignored_inside_function_in_condition
set -e
f() { false; echo "in f"; }
if f; then echo "f ok"; fi
f || echo unreachable
f && echo "and ok"
while f; do break; done
echo end
### expect
in f
f ok
in f
in f
and ok
in f
end
### end

### errexit_function_in_and_list_then_plain
### exit_code: 1
set -e
check() { echo "check $1"; [ "$1" = ok ]; echo "passed $1"; }
check ok
check bad && echo "and ran"
echo continue
check bad
echo never
### expect
check ok
passed ok
check bad
passed bad
and ran
continue
check bad
### end

### errexit_subshell_in_or_list
set -e
( false; echo "sub continues" ) || echo "sub failed $?"
echo end
### expect
sub continues
end
### end

### errexit_not_inherited_by_command_substitution
### exit_code: 3
set -e
x=$(false; echo "cmdsub continues")
echo "x=$x"
echo "flags=$(echo $- | tr -d hBc)"
shopt -s inherit_errexit
y=$(false; echo never) || echo "inherited rc=$?"
echo "y=[$y]"
v=$(exit 3)
echo never
### expect
x=cmdsub continues
flags=
y=[never]
### end

### errexit_inherited_cmdsub_exits_at_failure
### exit_code: 1
set -e
shopt -s inherit_errexit
f() { false; echo "in f"; }
x=$(f; echo more)
echo never
### expect
### end

### errexit_pipe_without_pipefail
set -e
false | true; echo "pipe ok"
### expect
pipe ok
### end

### errexit_child_shell_in_condition_keeps_its_own_set_e
# A child `bash` is a new process: the parent's if/||/! context does not
# disable `set -e` inside it (`if bash test.sh` harnesses rely on it).
f=/tmp/errexit-child.sh
printf '%s\n' 'set -e' 'g() { false; echo "g-after"; }' 'g' 'echo "script-end"' > "$f"
if bash "$f"; then echo "if: rc=0"; else echo "if: rc=$?"; fi
bash "$f" || echo "or: rc=$?"
! bash "$f" && echo "not: negated"
if bash -c 'set -e; false; echo c-after'; then echo "c: rc=0"; else echo "c: rc=$?"; fi
### expect
if: rc=1
or: rc=1
not: negated
c: rc=1
### end

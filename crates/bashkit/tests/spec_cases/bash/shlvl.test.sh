### shlvl_starts_at_one
# Cases end in `true`: the comparison harness runs real bash as `bash -c`,
# which execs its last simple command in place, while a bashkit script is a
# top-level script file, where nothing runs in place.
echo "$SHLVL"
### expect
1
### end

### shlvl_child_shell_increments
bash -c 'echo "a=$SHLVL"; true'
bash -c 'bash -c "echo b=\$SHLVL; true"; true'
true
### expect
a=2
b=3
### end

### shlvl_child_shell_increments_from_unset
unset SHLVL
bash -c 'echo "a=$SHLVL"; true'
bash -c 'bash -c "echo b=\$SHLVL; true"; true'
### expect
a=1
b=2
### end

### shlvl_child_shell_increments_an_inherited_value
export SHLVL=3
bash -c 'echo "c=$SHLVL"; true'
bash -c 'echo "d=${SHLVL}"; bash -c "echo e=\$SHLVL; true"; true'
true
### expect
c=4
d=4
e=5
### end

### shlvl_non_numeric_restarts_at_one
SHLVL=abc bash -c 'echo "f=$SHLVL"; true'
### expect
f=1
### end

### shlvl_too_high_resets_to_one
SHLVL=999 bash -c 'echo "g=$SHLVL"; true'
### expect
g=1
### end

### shlvl_negative_becomes_zero
SHLVL=-5 bash -c 'echo "h=$SHLVL"; true'
true
### expect
h=0
### end

### shlvl_is_exported_to_the_child
bash -c 'env | grep "^SHLVL=" ; true'
true
### expect
SHLVL=2
### end

### shlvl_nofork_last_command_of_c_string
# bash runs the last simple command of a `bash -c` string in place (no
# fork) and lowers SHLVL first, so a child shell started there keeps the
# parent's level: the right side of a trailing `&&`/`||`/`;` too.
unset SHLVL
bash -c 'bash -c "echo a=\$SHLVL"'
bash -c 'true && bash -c "echo b=\$SHLVL"'
bash -c 'false || bash -c "echo c=\$SHLVL"'
bash -c 'echo x >/dev/null; bash -c "echo d=\$SHLVL"'
bash -c 'true
bash -c "echo e=\$SHLVL"'
bash -c 'bash -c "echo f=\$SHLVL";'
bash -c 'command bash -c "echo g=\$SHLVL"'
bash -c 'X=1 bash -c "echo h=\$SHLVL"'
bash -c 'bash -c "bash -c \"echo i=\\\$SHLVL\""'
bash -c '{ true; } && bash -c "echo j=\$SHLVL"'
bash -c 'set -e; bash -c "echo k=\$SHLVL"'
true
### expect
a=1
b=1
c=1
d=1
e=1
f=1
g=1
h=1
i=1
j=1
k=1
### end

### shlvl_fork_kept_when_not_last_simple_command
# Only that last simple command is run in place: not one followed by more
# commands, a `{ }` group, a pipeline, a compound, a function, `!`, `eval`,
# one with redirections, or one after `&`.
unset SHLVL
bash -c 'bash -c "echo a=\$SHLVL"; true'
bash -c '{ bash -c "echo b=\$SHLVL"; }'
bash -c 'bash -c "echo c=\$SHLVL" | cat'
bash -c 'if true; then bash -c "echo d=\$SHLVL"; fi'
bash -c 'f() { bash -c "echo e=\$SHLVL"; }; f'
bash -c '! bash -c "echo f=\$SHLVL"'
bash -c 'bash -c "echo g=\$SHLVL" 2>/dev/null'
bash -c 'eval "bash -c \"echo h=\\\$SHLVL\""'
bash -c 'true & bash -c "echo i=\$SHLVL"; wait'
bash -c 'bash -c "echo j=\$SHLVL"
true'
bash -c 'bash -c "echo k=\$SHLVL" && true'
true
### expect
a=2
b=2
c=2
d=2
e=2
f=2
g=2
h=2
i=2
j=2
k=2
### end

### shlvl_fork_kept_when_traps_set
# A trap on EXIT or ERR, or a signal trap with a command, keeps the fork;
# an ignored signal, DEBUG, or a cleared trap does not.
unset SHLVL
bash -c 'trap "echo bye" EXIT; bash -c "echo a=\$SHLVL"'
bash -c 'trap "" EXIT; bash -c "echo b=\$SHLVL"'
bash -c 'trap "echo err" ERR; bash -c "echo c=\$SHLVL"'
bash -c 'trap "echo usr" USR1; bash -c "echo d=\$SHLVL"'
bash -c 'trap "" INT; bash -c "echo e=\$SHLVL"'
bash -c 'trap ":" DEBUG; bash -c "echo f=\$SHLVL"'
bash -c 'trap "echo t" EXIT; trap - EXIT; bash -c "echo g=\$SHLVL"'
true
### expect
a=2
bye
b=2
c=2
d=2
e=1
f=1
g=1
### end

### shlvl_nofork_in_subshells
# `( )`, `$( )`, backquotes and `<( )` run their last simple command in
# place as well. In `( )` and `<( )` a sole command may carry redirections;
# a substitution's may not, nor may the last of a list. A substitution
# expanded inside a pipeline keeps the level it was given.
export SHLVL=3
(bash -c 'echo a=$SHLVL')
(bash -c 'echo b=$SHLVL' 2>/dev/null)
(true; bash -c 'echo c=$SHLVL')
(true && bash -c 'echo d=$SHLVL')
(true; bash -c 'echo e=$SHLVL' 2>/dev/null)
(bash -c 'echo f=$SHLVL'; true)
echo "$(bash -c 'echo g=$SHLVL')"
echo "$(true; bash -c 'echo h=$SHLVL')"
echo "$(bash -c 'echo i=$SHLVL' 2>/dev/null)"
echo "`bash -c 'echo j=$SHLVL'`"
cat <(bash -c 'echo k=$SHLVL' 2>/dev/null)
(trap 'echo t' USR1; bash -c 'echo l=$SHLVL')
(bash -c 'echo m=$SHLVL') | cat
echo "$(bash -c 'echo n=$SHLVL')" | cat
trap 'echo bye' EXIT
(bash -c 'echo o=$SHLVL')
trap - EXIT
true
### expect
a=3
b=3
c=3
d=3
e=4
f=4
g=3
h=3
i=4
j=3
k=3
l=4
m=3
n=4
o=3
### end

### shlvl_exec_keeps_level
# `exec bash` lowers SHLVL first wherever it runs, except directly inside
# `( )` (a pipeline stage inside it lowers again).
export SHLVL=3
bash -c 'exec bash -c "echo a=\$SHLVL"'
bash -c 'echo x >/dev/null; exec bash -c "echo b=\$SHLVL"; true'
bash -c '{ exec bash -c "echo c=\$SHLVL"; }'
bash -c '(exec bash -c "echo d=\$SHLVL")'
(exec bash -c 'echo e=$SHLVL')
(exec bash -c 'echo f=$SHLVL' | cat)
echo "$(exec bash -c 'echo g=$SHLVL')"
(echo "$(exec bash -c 'echo h=$SHLVL')")
bash -c 'f() { exec bash -c "echo i=\$SHLVL"; }; f'
true
### expect
a=4
b=4
c=4
d=5
e=4
f=3
g=3
h=4
i=4
### end

### shlvl_background_simple_command_keeps_level
# A simple command run with `&` is run in place in its forked child.
export SHLVL=3
bash -c 'echo a=$SHLVL' & wait
bash -c 'echo b=$SHLVL' 2>/dev/null & wait
{ bash -c 'echo c=$SHLVL'; } & wait
(bash -c 'echo d=$SHLVL') & wait
bash -c 'bash -c "echo e=\$SHLVL" & wait'
true
### expect
a=3
b=3
c=4
d=3
e=4
### end

### shlvl_nofork_level_bounds
# The in-place command lowers the shell's own SHLVL, exported or not, and
# clamps at 0 before the child counts up.
bash -c 'SHLVL=-5; bash -c "echo a=\$SHLVL"'
bash -c 'SHLVL=abc; bash -c "echo b=\$SHLVL"'
bash -c 'export -n SHLVL; SHLVL=7; bash -c "echo c=\$SHLVL"'
bash -c 'unset SHLVL; bash -c "echo d=\$SHLVL"'
bash -c 'SHLVL=0; bash -c "echo e=\$SHLVL"'
true
### expect
a=1
b=1
c=7
d=1
e=1
### end

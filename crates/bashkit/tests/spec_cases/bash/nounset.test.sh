### nounset_check_flag
# set -u sets SHOPT_u
### bash_diff: SHOPT_u is bashkit-internal variable
set -u
echo "SHOPT_u=$SHOPT_u"
### expect
SHOPT_u=1
### end

### nounset_unset_var_error
# set -u aborts on unset variables
### bash_diff: real bash prints to stderr and exits shell
### exit_code:1
set -u
echo $UNDEFINED_VAR_XYZ
echo "should not reach"
### expect
### end

### nounset_set_var_ok
# set -u allows set variables
set -u
MY_VAR=hello
echo "$MY_VAR"
### expect
hello
### end

### nounset_special_vars
# set -u allows special variables
set -u
echo "$?"
### expect
0
### end

### nounset_empty_var_ok
# set -u allows empty but set variables
set -u
EMPTY=""
echo "value=$EMPTY"
### expect
value=
### end

### nounset_default_value_ok
# ${var:-default} should not error under set -u
set -u
echo "${UNDEFINED_XYZ:-fallback}"
### expect
fallback
### end

### nounset_disable
# set +u disables nounset
set -u
set +u
echo "$UNDEFINED_VAR_XYZ"
echo "ok"
### expect

ok
### end

### nounset_error_in_function_exits_script
# ${x?msg} inside a function exits the whole non-interactive shell
# (a subshell only ends itself)
### bash_diff: bash -c exits 127 after this error; a script file exits 1
### exit_code:1
f(){ : ${zz?y}; echo in; }
(f; echo x); echo "status $?"
g(){ f; echo g; }
g
echo after
### expect
status 1
### end

### nounset_unbound_in_function_exits_script
# set -u: an unbound variable inside a function exits the script
### bash_diff: bash -c exits 127 after this error; a script file exits 1
### exit_code:1
set -u
f(){ x=$zz; echo in; }
if f; then echo then; fi
echo after
### expect
### end

### nounset_error_inside_command_substitution_reported
# the unbound-variable error raised inside $(...) reaches the shell's stderr
set -u
exec 2>&1
x=$(echo $zz); echo "r=$?"
: "$(echo $zz)"; echo "r=$?"
y=$(: ${zz?inner}; echo no); echo "[$y]"
ls /nonexistent-dir >/dev/null; echo after
### expect
bash: line 3: zz: unbound variable
r=1
bash: line 4: zz: unbound variable
r=0
bash: line 5: zz: inner
[]
ls: cannot access '/nonexistent-dir': No such file or directory
after
### end

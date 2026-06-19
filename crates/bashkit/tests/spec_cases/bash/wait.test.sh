### wait_basic
### bash_diff: VFS runs background jobs synchronously
# wait returns success
wait
echo $?
### expect
0
### end

### wait_with_pid
### bash_diff: VFS runs background jobs synchronously
# wait with a PID argument
echo "hello" &
wait $!
echo $?
### expect
hello
0
### end

### bang_var_basic
# $! is set to the most recent background job id (non-empty)
true &
[ -n "$!" ] && echo set || echo unset
### expect
set
### end

### bang_var_isolated_across_subshell
### bash_diff: VFS runs background jobs synchronously
# A $(...) subshell starting its own background job must not change the
# parent's $! (last background PID is per-shell state, like real bash).
true &
parent=$!
junk=$(true & true)
[ "$!" = "$parent" ] && echo isolated || echo leaked
### expect
isolated
### end

### bg_internal_names_are_ordinary_vars
### bash_diff: VFS runs background jobs synchronously
# _LAST_BG_PID / _BG_EXIT_CODE are no longer interpreter channels in the
# variable namespace; backgrounding tracks $! in typed state and leaves these
# user variables untouched.
_LAST_BG_PID=hijack
_BG_EXIT_CODE=hijack2
true &
echo "$_LAST_BG_PID $_BG_EXIT_CODE"
### expect
hijack hijack2
### end

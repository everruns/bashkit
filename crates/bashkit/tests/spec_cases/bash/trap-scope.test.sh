# ERR and RETURN trap scoping (bash 5.2 semantics): which commands fire ERR,
# inheritance by functions/subshells (set -E / set -T), RETURN after functions
# and sourced files.

### err_trap_compound_commands_do_not_fire_it
# the failing command inside a group, loop or case fires ERR, not the compound
trap 'echo err' ERR
{ false; echo x; }
for i in 1; do false; done
case a in a) false;; esac
if true; then false; fi
echo end
### expect
err
x
err
err
err
end
### end

### err_trap_test_commands_and_pipelines_fire_it
# [[ ]], (( )) and a failing pipeline fire ERR once; ! and && heads do not
trap 'echo err' ERR
[[ a = b ]]
((0))
true | false
false | true
! false
false && true
true && false
echo end
### expect
err
err
err
err
end
### end

### err_trap_eval_fires_inside_and_for_itself
# eval fires for the failing command it runs and for its own status
trap 'echo err' ERR
eval false
eval 'false; true'
echo end
### expect
err
err
err
end
### end

### err_trap_pipeline_stage_is_a_subshell
# a pipeline stage keeps the ERR trap dormant unless set -E
trap 'echo err' ERR
{ false; echo x; } | cat
set -E
{ false; echo y; } | cat
### expect
x
err
y
### end

### err_trap_listed_in_a_subshell
# a subshell lists the inherited ERR trap without running it; one it sets runs
trap 'echo err' ERR
(trap -p ERR; false; echo in)
x=$(trap -p ERR)
echo "[$x]"
(trap 'echo sub' ERR; false; echo z)
### expect
trap -- 'echo err' ERR
in
[trap -- 'echo err' ERR]
sub
z
### end

### err_trap_function_restores_the_callers_trap
# a function that resets ERR gets the caller's back; one that sets it keeps its own
trap 'echo A' ERR
f() { trap - ERR; return 1; }
f
trap -p ERR
g() { trap 'echo B' ERR; return 1; }
g
trap -p ERR
### expect
A
trap -- 'echo A' ERR
B
trap -- 'echo B' ERR
### end

### err_trap_lineno_is_the_failing_line
# $LINENO in the handler is the line of the failing command or call
trap 'echo "err $LINENO"' ERR
f() {
  false
}
f
false
### expect
err 5
err 6
### end

### return_trap_runs_after_source
# the RETURN trap runs when a sourced file finishes, and in functions only with set -T
echo 'echo in' > ret_src.sh
trap 'echo "ret ${FUNCNAME[0]:-top}"' RETURN
. ./ret_src.sh
f() { . ./ret_src.sh; }
f
set -T
f
### expect
in
ret top
in
in
ret f
ret f
### end

### return_trap_keeps_status
# the RETURN handler sees $? and leaves the function status alone
f() { trap 'echo "ret $?"' RETURN; false; }
f
echo "status $?"
### expect
ret 1
status 1
### end

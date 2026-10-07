# BashBox err-trap cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_err_trap_functions_do_not_inherit_the_err_trap
# functions do not inherit the ERR trap
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
trap 'echo err' ERR; f(){ false; echo in; }; f; echo out
### expect
in
out
### end

### bashbox_err_trap_the_failing_call_still_fires_it
# the failing call still fires it
trap 'echo err' ERR; f(){ g; }; g(){ false; }; f; echo end
### expect
err
end
### end

### bashbox_err_trap_set_e_passes_it_to_functions
# set -E passes it to functions
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -E; trap 'echo err $LINENO' ERR; f(){ false; }; f; echo $-; set +E; f; echo end
### expect
err 1
err 1
hBEc
err 1
end
### end

### bashbox_err_trap_a_trap_set_in_a_function_stays
# a trap set in a function stays
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ trap 'echo ferr $LINENO' ERR; false; }
f
f
false
echo end
### expect
ferr 1
ferr 1
ferr 3
ferr 4
end
### end

### bashbox_err_trap_the_call_fires_only_a_trap_set_before_it
# the call fires only a trap set before it
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ trap 'echo t' ERR; return 1; }; f; false; echo end
### expect
t
end
### end

### bashbox_err_trap_eval_fires_the_trap_it_found
# eval fires the trap it found
trap 'echo a' ERR; eval false; eval "trap 'echo b' ERR; false"; echo end
### expect
a
a
b
b
end
### end

### bashbox_err_trap_conditions_do_not_fire_it
# conditions do not fire it
trap 'echo err' ERR; if false; then :; elif false; then :; fi; while false; do :; done; until true; do :; done; echo end
### expect
end
### end

### bashbox_err_trap_tested_function_calls_do_not_fire_it
# tested function calls do not fire it
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
trap 'echo err' ERR; f(){ false; echo in; }; f || true; f && true; ! f; if f; then :; fi; echo end
### expect
in
in
in
in
end
### end

### bashbox_err_trap_and_or_lists_fire_only_for_the_last
# and-or lists fire only for the last
trap 'echo err' ERR; false && true; false || false; echo end
### expect
err
end
### end

### bashbox_err_trap_compound_commands_fire_only_through_their_commands
# compound commands fire only through their commands
trap 'echo err' ERR; { false; }; if true; then false; fi; for i in 1; do false; done; case x in x) false;; esac; echo end
### expect
err
err
err
err
end
### end

### bashbox_err_trap_a_failing_subshell_fires_it_once
# a failing subshell fires it once
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
trap 'echo err' ERR; (false); (false; true); echo end
### expect
err
end
### end

### bashbox_err_trap_set_e_reaches_subshells
# set -E reaches subshells
set -E; trap 'echo err' ERR; (false; true); echo end
### expect
err
end
### end

### bashbox_err_trap_command_substitution_drops_the_err_trap
# command substitution drops the ERR trap
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
trap 'echo err' ERR; x=$(false; echo in); echo "[$x]"; f(){ false; }; y=$(f); echo "[$y]"
### expect
[in]
err
[]
### end

### bashbox_err_trap_set_e_keeps_it_in_command_substitution
# set -E keeps it in command substitution
set -E; trap 'echo err' ERR; x=$(false; echo in); echo "[$x]"
### expect
[err
in]
### end

### bashbox_err_trap_arithmetic_and_fire_it
# arithmetic and [[ ]] fire it
trap 'echo err' ERR; [[ 1 = 2 ]]; (( 0 )); echo end
### expect
err
err
end
### end

### bashbox_err_trap_errexit_ignores_conditions
# errexit ignores conditions
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -e; if false; then :; fi; while false; do :; done; f(){ false; echo in; }; f || true; f && true; ! f; echo end
### expect
in
in
in
end
### end

### bashbox_err_trap_set_t_passes_the_return_trap_to_functions
# set -T passes the RETURN trap to functions
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -T; echo $-; trap 'echo r' RETURN; g(){ :; }; f(){ g; }; f; echo end
### expect
hBTc
r
r
end
### end

### bashbox_err_trap_without_t_it_stays_out_of_nested_calls
# without -T it stays out of nested calls
trap 'echo r' RETURN; g(){ :; }; f(){ g; }; f; echo end
### expect
end
### end

### bashbox_err_trap_set_o_lists_the_options
# set -o lists the options
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -o | grep -E "errtrace|functrace"; set -E; set +o | grep errtrace
### expect
errtrace       	off
functrace      	off
set -o errtrace
### end

# BashBox syntax-error cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_syntax_error_eval_reports_and_returns_2
# eval reports and returns 2
eval fi; echo $?
### expect
2
### end

### bashbox_syntax_error_a_sourced_file_reports_under_its_name_and_returns_2
# a sourced file reports under its name and returns 2
echo 'if then' > bad.sh; source ./bad.sh; echo $?
### expect
2
### end

### bashbox_syntax_error_a_backquoted_substitution_is_parsed_when_it_runs_and_fails_w
# a backquoted substitution is parsed when it runs and fails with 2
x=`fi`; echo "[$x] $?"
### expect
[] 2
### end

### bashbox_syntax_error_the_exit_trap_reports_its_own_syntax_error
# the EXIT trap reports its own syntax error
trap fi EXIT; echo a
### expect
a
### end

### bashbox_syntax_error_the_err_trap_reports_its_own_syntax_error
# the ERR trap reports its own syntax error
trap fi ERR; false; echo $?
### expect
1
### end

### bashbox_syntax_error_the_return_trap_reports_its_own_syntax_error
# the RETURN trap reports its own syntax error
f(){ trap fi RETURN; }; f; echo $?
### expect
0
### end

### bashbox_syntax_error_a_case_pattern
# a case pattern
x=$(case a in a) echo A;; esac); echo "[$x]"
### expect
[A]
### end

### bashbox_syntax_error_a_case_pattern_in_double_quotes
# a case pattern in double quotes
echo "$(case b in a) echo A;; b) echo B;; esac)"
### expect
B
### end

### bashbox_syntax_error_a_comment
# a comment
x=$( # comment with )
echo hi); echo "[$x]"
### expect
[hi]
### end

### bashbox_syntax_error_a_comment_after_a_command
# a comment after a command
x=$(echo a #)
); echo $x
### expect
a
### end

### bashbox_syntax_error_a_here_document
# a here-document
x=$(cat <<EOF
)
EOF
); echo "[$x]"
### expect
[)]
### end

### bashbox_syntax_error_a_process_substitution
# a process substitution
cat <(case a in a) echo P;; esac)
### expect
P
### end

### bashbox_syntax_error_nested_substitutions
# nested substitutions
echo $(echo $(case a in a) echo in;; esac) out)
### expect
in out
### end

### bashbox_syntax_error_arithmetic_still_counts_parentheses
# arithmetic still counts parentheses
echo $(( (1 + 2) * 3 ))
### expect
9
### end

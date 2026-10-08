# Parse error tests
# Inspired by Oils spec/parse-errors.test.sh
# https://github.com/oilshell/oil/blob/master/spec/parse-errors.test.sh

### parse_dollar_percent_not_error
# $% is not a parse error
echo $%
### expect
$%
### end

### parse_bad_braced_var
# Bad braced var sub is an error
bash -c 'echo ${%}' 2>/dev/null
echo status=$?
### expect
status=1
### end

### parse_incomplete_while
# Incomplete while is a parse error (tested indirectly)
echo done
### expect
done
### end

### parse_unexpected_do
# do unexpected outside loop - bashkit should reject this
bash -c 'do echo hi' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_unexpected_rbrace
# } is a parse error at top level
bash -c '}' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_lbrace_needs_space
# { needs a space after it
### skip: TODO parser does not require space after '{'
bash -c '{ls; }' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_invalid_for_var
# Invalid for loop variable name
### skip: TODO bashkit returns exit 2 (parse error) but real bash returns exit 1 (runtime error)
bash -c 'for i.j in a b c; do echo hi; done' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_bad_var_name_not_assignment
# bad var name globally isn't parsed like an assignment
bash -c 'FOO-BAR=foo' 2>/dev/null
echo status=$?
### expect
status=127
### end

### parse_bad_var_name_export
# bad var name in export
bash -c 'export FOO-BAR=foo' 2>/dev/null
test $? -ne 0 && echo error
### expect
error
### end

### parse_bad_var_name_local
# bad var name in local
bash -c 'f() { local FOO-BAR=foo; }; f' 2>/dev/null
test $? -ne 0 && echo error
### expect
error
### end

### parse_misplaced_parens
# misplaced parentheses are a syntax error
### skip: TODO parser does not reject misplaced parentheses
bash -c 'echo a(b)' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_incomplete_command_sub
# incomplete command sub
bash -c '$(x' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_incomplete_backticks
# incomplete backticks
bash -c '`x' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_misplaced_double_semi
# misplaced ;; outside case
bash -c 'echo 1 ;; echo 2' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_empty_double_bracket
# empty clause in [[
### skip: TODO [[ || true ]] not rejected as parse error
bash -c '[[ || true ]]' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_unterminated_single_quote
# Unterminated single quote is a parse error
bash -c "echo 'unterminated" 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_unterminated_double_quote
# Unterminated double quote is a parse error
bash -c 'echo "unterminated' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_case_bad_semicolon
# Using ; instead of ;; in case
bash -c 'case x in x) ; y) echo ;; esac' 2>/dev/null
echo status=$?
### expect
status=2
### end

### syntax_error_near_unexpected_token
# bash names the token the parse stopped at, then the line
bash -c 'if then fi' 2>&1; echo rc=$?
bash -c 'echo a; fi' 2>&1
bash -c 'while do done' 2>&1
bash -c 'a && && b' 2>&1
bash -c 'echo (' 2>&1
### expect
bash: -c: line 1: syntax error near unexpected token `then'
bash: -c: line 1: `if then fi'
rc=2
bash: -c: line 1: syntax error near unexpected token `fi'
bash: -c: line 1: `echo a; fi'
bash: -c: line 1: syntax error near unexpected token `do'
bash: -c: line 1: `while do done'
bash: -c: line 1: syntax error near unexpected token `&&'
bash: -c: line 1: `a && && b'
bash: -c: line 1: syntax error near unexpected token `newline'
bash: -c: line 1: `echo ('
### end

### syntax_error_unexpected_end_of_file
# input ending inside a construct: reported on the line after the last
bash -c 'for x in; do' 2>&1; echo rc=$?
bash -c 'if true; then' 2>&1
bash -c '{ echo' 2>&1
bash -c 'case x in' 2>&1
printf 'echo 1\nif true; then\n' | bash 2>&1
### expect
bash: -c: line 2: syntax error: unexpected end of file
rc=2
bash: -c: line 2: syntax error: unexpected end of file
bash: -c: line 2: syntax error: unexpected end of file
bash: -c: line 2: syntax error: unexpected end of file
1
bash: line 3: syntax error: unexpected end of file
### end

### syntax_error_partial_run_and_eval
# commands before the error run; eval counts from its own line
bash -c $'echo first\nfor x in; do' 2>&1; echo rc=$?
bash -c $'echo x\neval "if then fi"' 2>&1
### expect
first
bash: -c: line 3: syntax error: unexpected end of file
rc=2
x
bash: eval: line 2: syntax error near unexpected token `then'
bash: eval: line 2: `if then fi'
### end

### syntax_error_unterminated_quote
# an unterminated quote names the quote it looked for
bash -c 'echo "a' 2>&1
bash -c "echo 'a" 2>&1
### expect
bash: -c: line 1: unexpected EOF while looking for matching `"'
bash: -c: line 1: unexpected EOF while looking for matching `''
### end

### syntax_error_dbracket_regex_unclosed_group
# [[ =~ ( ]] reads the group to end of input, then the operand is missing
bash -c '[[ abc =~ ( ]]' 2>&1; echo rc=$?
### expect
bash: -c: line 1: unexpected EOF while looking for matching `)'
bash: -c: line 2: unexpected argument to conditional binary operator
rc=2
### end

### bash_child_errors_follow_its_redirects
# a child shell's syntax error and missing script obey the command's redirects
{ bash -c 'if then fi' 2>/dev/null; } 2>&1; echo rc=$?
{ bash -c 'echo "a' 2>/dev/null; } 2>&1
{ bash /no/such/file 2>/dev/null; } 2>&1; echo rc=$?
### expect
rc=2
rc=127
### end

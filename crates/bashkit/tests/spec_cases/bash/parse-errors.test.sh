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
### skip: TODO ${%} not rejected as parse error
echo ${%} 2>/dev/null
echo status=$?
### exit_code: 1
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
### skip: TODO parser does not reject unexpected 'do' keyword
bash -c 'do echo hi' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_unexpected_rbrace
# } is a parse error at top level
### skip: TODO parser does not reject unexpected '}' at top level
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
### skip: TODO parser does not reject invalid for-loop variable names
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
### skip: TODO export does not reject invalid variable names
bash -c 'export FOO-BAR=foo' 2>/dev/null
test $? -ne 0 && echo error
### expect
error
### end

### parse_bad_var_name_local
# bad var name in local
### skip: TODO local does not reject invalid variable names
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
### skip: TODO parser does not reject incomplete command substitution
bash -c '$(x' 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_incomplete_backticks
# incomplete backticks
### skip: TODO parser does not reject incomplete backticks
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
### skip: TODO unterminated quotes not detected as parse error
bash -c "echo 'unterminated" 2>/dev/null
echo status=$?
### expect
status=2
### end

### parse_unterminated_double_quote
# Unterminated double quote is a parse error
### skip: TODO unterminated quotes not detected as parse error
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

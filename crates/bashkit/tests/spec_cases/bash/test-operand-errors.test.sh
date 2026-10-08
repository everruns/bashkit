### integer_operand_must_be_an_integer
### exit_code: 2
test 1 -eq abc 2>/dev/null
### end

### bracket_integer_operand_must_be_an_integer
### exit_code: 2
[ a -lt 2 ] 2>/dev/null
### end

### missing_operand_is_a_unary_operator_error
### exit_code: 2
[ a -lt ] 2>/dev/null
### end

### two_plain_words_are_a_unary_operator_error
### exit_code: 2
[ x y ] 2>/dev/null
### end

### unknown_binary_operator_is_an_error
### exit_code: 2
test a foo b 2>/dev/null
### end

### four_operands_are_too_many
### exit_code: 2
test a b c d 2>/dev/null
### end

### a_connective_checks_both_sides
### exit_code: 2
[ 1 -eq 2 -a abc -eq 1 ] 2>/dev/null
### end

### a_base_prefix_is_not_an_integer_here
### exit_code: 2
test 0x10 -eq 16 2>/dev/null
### end

### whitespace_and_sign_and_leading_zeros_are_fine
test " 5 " -eq 5 && test +5 -eq 5 && test 010 -eq 10 && echo ok
### expect
ok
### end

### double_bracket_still_answers_false
[[ 1 -eq abc ]]; echo "rc=$?"
### expect
rc=1
### end

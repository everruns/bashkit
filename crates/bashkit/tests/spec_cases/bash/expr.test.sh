### expr_add
# expr addition
expr 2 + 3
### expect
5
### end

### expr_subtract
# expr subtraction
expr 10 - 3
### expect
7
### end

### expr_multiply
# expr multiplication (note: * must be escaped in shell)
expr 4 \* 5
### expect
20
### end

### expr_divide
# expr integer division
expr 10 / 3
### expect
3
### end

### expr_modulo
# expr modulo
expr 10 % 3
### expect
1
### end

### expr_length
# expr string length
expr length "hello"
### expect
5
### end

### expr_substr
# expr substring extraction (1-based)
expr substr "hello" 2 3
### expect
ell
### end

### expr_equal
# expr equality comparison
expr "abc" = "abc"
### expect
1
### end

### expr_not_equal
# expr inequality
expr "abc" != "def"
### expect
1
### end

### expr_less_than
# expr less than (numeric)
expr 3 \< 5
### expect
1
### end

### expr_pattern_match
# expr pattern match returns match length
expr "hello" : "hel"
### expect
3
### end

### expr_no_args
# expr with no args returns error
expr 2>/dev/null
echo "exit:$?"
### expect
exit:2
### end

### expr_zero_exit
# expr returns exit 1 for zero result
expr 0 + 0
echo "exit:$?"
### expect
0
exit:1
### end

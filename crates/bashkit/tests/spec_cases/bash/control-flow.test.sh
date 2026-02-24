### if_true
# If with true condition
if true; then echo yes; fi
### expect
yes
### end

### if_false
# If with false condition
if false; then echo yes; fi
### expect
### end

### if_else
# If-else
if false; then echo yes; else echo no; fi
### expect
no
### end

### if_elif
# If-elif-else chain
if false; then echo one; elif true; then echo two; else echo three; fi
### expect
two
### end

### if_test_eq
# If with numeric equality
if [ 5 -eq 5 ]; then echo equal; fi
### expect
equal
### end

### if_test_ne
# If with numeric inequality
if [ 5 -ne 3 ]; then echo different; fi
### expect
different
### end

### if_test_gt
# If with greater than
if [ 5 -gt 3 ]; then echo bigger; fi
### expect
bigger
### end

### if_test_lt
# If with less than
if [ 3 -lt 5 ]; then echo smaller; fi
### expect
smaller
### end

### if_test_string_eq
# If with string equality
if [ foo = foo ]; then echo match; fi
### expect
match
### end

### if_test_string_ne
# If with string inequality
if [ foo != bar ]; then echo different; fi
### expect
different
### end

### if_test_z
# If with empty string test
if [ -z "" ]; then echo empty; fi
### expect
empty
### end

### if_test_n
# If with non-empty string test
if [ -n "hello" ]; then echo nonempty; fi
### expect
nonempty
### end

### for_simple
# Simple for loop
for i in a b c; do echo $i; done
### expect
a
b
c
### end

### for_numbers
# For loop with numbers
for i in 1 2 3; do echo $i; done
### expect
1
2
3
### end

### for_with_break
# For loop with break
for i in a b c; do echo $i; break; done
### expect
a
### end

### for_with_continue
# For loop with continue
for i in 1 2 3; do if [ $i -eq 2 ]; then continue; fi; echo $i; done
### expect
1
3
### end

### while_counter
# While loop with counter
i=0; while [ $i -lt 3 ]; do echo $i; i=$((i + 1)); done
### expect
0
1
2
### end

### while_false
# While with false condition
while false; do echo loop; done; echo done
### expect
done
### end

### while_break
# While with break
i=0; while [ $i -lt 10 ]; do echo $i; i=$((i + 1)); if [ $i -ge 3 ]; then break; fi; done
### expect
0
1
2
### end

### case_literal
# Case with literal match
case foo in foo) echo matched;; esac
### expect
matched
### end

### case_wildcard
# Case with wildcard
case bar in *) echo default;; esac
### expect
default
### end

### case_multiple
# Case with multiple patterns
case foo in bar|foo|baz) echo matched;; esac
### expect
matched
### end

### case_no_match
# Case with no match
case foo in bar) echo no;; esac
### expect
### end

### case_pattern
# Case with glob pattern
case hello in hel*) echo prefix;; esac
### expect
prefix
### end

### and_list_success
# AND list with success
true && echo yes
### expect
yes
### end

### and_list_failure
# AND list short-circuit
false && echo no
### exit_code: 1
### expect
### end

### or_list_success
# OR list short-circuit
true || echo no
### expect
### end

### or_list_failure
# OR list with failure
false || echo fallback
### expect
fallback
### end

### command_list
# Semicolon command list
echo one; echo two; echo three
### expect
one
two
three
### end

### subshell
# Subshell execution
(echo hello)
### expect
hello
### end

### subshell_redirect
# Subshell with output redirection
(echo redirected) > /tmp/subshell_out.txt && cat /tmp/subshell_out.txt
### expect
redirected
### end

### brace_group
# Brace group
{ echo hello; }
### expect
hello
### end

### arith_for_le
# C-style for loop with <= condition
for ((i=1; i<=3; i++)); do echo $i; done
### expect
1
2
3
### end

### arith_for_ge
# C-style for loop with >= (countdown)
for ((i=3; i>=1; i--)); do echo $i; done
### expect
3
2
1
### end

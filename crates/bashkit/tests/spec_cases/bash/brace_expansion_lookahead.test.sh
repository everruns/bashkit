# Brace expansion lookahead limit
# Regression tests for issue #997

### normal_brace_expansion
# Normal brace expansion still works
echo {a,b,c}
### expect
a b c
### end

### range_brace_expansion
# Range brace expansion still works
echo {1..3}
### expect
1 2 3
### end

### unmatched_brace_literal
# Unmatched { is treated as literal
echo {abc
### expect
{abc
### end

### nested_brace_expansion
# Nested brace expansion works
echo {a,{b,c}}
### expect
a b c
### end

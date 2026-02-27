# Dynamic arithmetic tests
# Inspired by Oils spec/arith-dynamic.test.sh
# https://github.com/oilshell/oil/blob/master/spec/arith-dynamic.test.sh

### arith_dyn_var_reference
# Variable references in arithmetic
x='1'
echo $(( x + 2 * 3 ))
### expect
7
### end

### arith_dyn_var_expression
# Variable containing expression is re-evaluated
### skip: TODO arithmetic variable re-evaluation of expressions not implemented
x='1 + 2'
echo $(( x * 3 ))
### expect
9
### end

### arith_dyn_substitution
# $x in arithmetic
x='1 + 2'
echo $(( $x * 3 ))
### expect
7
### end

### arith_dyn_quoted_substitution
# "$x" in arithmetic
### skip: TODO double-quoted substitution in arithmetic not implemented
x='1 + 2'
echo $(( "$x" * 3 ))
### expect
7
### end

### arith_dyn_nested_var
# Nested variable reference in arithmetic
### skip: TODO recursive variable dereferencing in arithmetic not implemented
a=3
b=a
echo $(( b + 1 ))
### expect
4
### end

### arith_dyn_array_index
# Dynamic array index
### skip: TODO array access in arithmetic expressions not implemented
arr=(10 20 30 40)
i=2
echo $(( arr[i] ))
### expect
30
### end

### arith_dyn_array_index_expr
# Expression as array index
### skip: TODO array access in arithmetic expressions not implemented
arr=(10 20 30 40)
echo $(( arr[1+1] ))
### expect
30
### end

### arith_dyn_conditional
# Ternary operator
x=5
echo $(( x > 3 ? 1 : 0 ))
echo $(( x < 3 ? 1 : 0 ))
### expect
1
0
### end

### arith_dyn_comma
# Comma operator
echo $(( 1, 2, 3 ))
### expect
3
### end

### arith_dyn_assign_in_expr
# Assignment within arithmetic expression
echo $(( x = 5 + 3 ))
echo $x
### expect
8
8
### end

### arith_dyn_pre_post_increment
# Pre/post increment in dynamic context
x=5
echo $(( x++ ))
echo $x
echo $(( ++x ))
echo $x
### expect
5
6
7
7
### end

### arith_dyn_compound_assign
# Compound assignment operators
x=10
echo $(( x += 5 ))
echo $(( x -= 3 ))
echo $(( x *= 2 ))
echo $(( x /= 4 ))
echo $(( x %= 2 ))
### expect
15
12
24
6
0
### end

### arith_dyn_unset_var_is_zero
# Unset variable in arithmetic treated as 0
unset arith_undef_xyz
echo $(( arith_undef_xyz + 5 ))
### expect
5
### end

### arith_dyn_string_var_is_zero
# Non-numeric string in arithmetic treated as 0
x=hello
echo $(( x + 5 ))
### expect
5
### end

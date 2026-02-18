### arith_add
# Simple addition
echo $((1 + 2))
### expect
3
### end

### arith_subtract
# Subtraction
echo $((5 - 3))
### expect
2
### end

### arith_multiply
# Multiplication
echo $((3 * 4))
### expect
12
### end

### arith_divide
# Division
echo $((10 / 2))
### expect
5
### end

### arith_modulo
# Modulo
echo $((10 % 3))
### expect
1
### end

### arith_precedence
# Operator precedence
echo $((2 + 3 * 4))
### expect
14
### end

### arith_parens
# Parentheses
echo $(((2 + 3) * 4))
### expect
20
### end

### arith_negative
# Negative numbers
echo $((-5 + 3))
### expect
-2
### end

### arith_variable
# With variable
X=5; echo $((X + 3))
### expect
8
### end

### arith_variable_dollar
# With $variable
X=5; echo $(($X + 3))
### expect
8
### end

### arith_compare_eq
# Comparison equal
echo $((5 == 5))
### expect
1
### end

### arith_compare_ne
# Comparison not equal
echo $((5 != 3))
### expect
1
### end

### arith_compare_gt
# Comparison greater
echo $((5 > 3))
### expect
1
### end

### arith_compare_lt
# Comparison less
echo $((3 < 5))
### expect
1
### end

### arith_increment
# Increment
X=5; echo $((X + 1))
### expect
6
### end

### arith_decrement
# Decrement
X=5; echo $((X - 1))
### expect
4
### end

### arith_compound
# Compound expression
echo $((1 + 2 + 3 + 4))
### expect
10
### end

### arith_assign
# Assignment in arithmetic
X=5; echo $((X = X + 1)); echo $X
### expect
6
6
### end

### arith_complex
# Complex expression
A=2; B=3; echo $(((A + B) * (A - B) + 10))
### expect
5
### end

### arith_ternary
# Ternary operator
echo $((5 > 3 ? 1 : 0))
### expect
1
### end

### arith_bitwise_and
# Bitwise AND
echo $((5 & 3))
### expect
1
### end

### arith_bitwise_or
# Bitwise OR
echo $((5 | 3))
### expect
7
### end

### arith_logical_and_true
# Logical AND - both true
echo $((1 && 1))
### expect
1
### end

### arith_logical_and_false
# Logical AND - second false
echo $((1 && 0))
### expect
0
### end

### arith_logical_and_first_false
# Logical AND - first false (short-circuit)
echo $((0 && 1))
### expect
0
### end

### arith_logical_or_true
# Logical OR - first true (short-circuit)
echo $((1 || 0))
### expect
1
### end

### arith_logical_or_false
# Logical OR - both false
echo $((0 || 0))
### expect
0
### end

### arith_logical_or_second_true
# Logical OR - first false, second true
echo $((0 || 1))
### expect
1
### end

### arith_logical_combined
# Combined logical operators
echo $((1 || 0 && 0))
### expect
1
### end

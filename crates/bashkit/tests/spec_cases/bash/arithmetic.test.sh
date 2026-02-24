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

### arith_exponentiation
# ** power operator
echo $((2 ** 10))
### expect
1024
### end

### arith_exponentiation_variable
# ** with variable
x=5; echo $(( x ** 2 ))
### expect
25
### end

### arith_base_hex
# Base conversion: 16#ff = 255
echo $((16#ff))
### expect
255
### end

### arith_base_binary
# Base conversion: 2#1010 = 10
echo $((2#1010))
### expect
10
### end

### arith_base_octal
# Base conversion: 8#77 = 63
echo $((8#77))
### expect
63
### end

### arith_hex_literal
# 0x hex literal
echo $((0xff))
### expect
255
### end

### arith_octal_literal
# Octal literal
echo $((077))
### expect
63
### end

### arith_unary_negate
# Unary negation
echo $((-5))
### expect
-5
### end

### arith_bitwise_not
# Bitwise NOT
echo $((~0))
### expect
-1
### end

### arith_logical_not
# Logical NOT
echo $((!0))
### expect
1
### end

### arith_bitwise_xor
# Bitwise XOR
echo $((5 ^ 3))
### expect
6
### end

### arith_shift_left
# Left shift
echo $((1 << 4))
### expect
16
### end

### arith_shift_right
# Right shift
echo $((16 >> 2))
### expect
4
### end

### arith_compound_add_assign
# Compound += assignment
x=10; echo $(( x += 5 ))
### expect
15
### end

### arith_compound_sub_assign
# Compound -= assignment
x=10; echo $(( x -= 3 ))
### expect
7
### end

### arith_compound_mul_assign
# Compound *= assignment
x=4; echo $(( x *= 3 ))
### expect
12
### end

### arith_compound_and_assign
# Compound &= assignment
x=7; echo $(( x &= 3 ))
### expect
3
### end

### arith_compound_or_assign
# Compound |= assignment
x=5; echo $(( x |= 2 ))
### expect
7
### end

### arith_compound_xor_assign
# Compound ^= assignment
x=5; echo $(( x ^= 3 ))
### expect
6
### end

### arith_compound_shl_assign
# Compound <<= assignment
x=1; echo $(( x <<= 4 ))
### expect
16
### end

### arith_compound_shr_assign
# Compound >>= assignment
x=16; echo $(( x >>= 2 ))
### expect
4
### end

### arith_pre_increment
# Pre-increment ++var
x=5; echo $(( ++x )); echo $x
### expect
6
6
### end

### arith_post_increment
# Post-increment var++
x=5; echo $(( x++ )); echo $x
### expect
5
6
### end

### arith_pre_decrement
# Pre-decrement --var
x=5; echo $(( --x )); echo $x
### expect
4
4
### end

### arith_post_decrement
# Post-decrement var--
x=5; echo $(( x-- )); echo $x
### expect
5
4
### end

### arith_comma_operator
# Comma operator (evaluate all, return last)
echo $(( x=3, y=4, x+y ))
### expect
7
### end

### arith_compare_le
# Less than or equal
echo $((3 <= 5)); echo $((5 <= 5)); echo $((6 <= 5))
### expect
1
1
0
### end

### arith_compare_ge
# Greater than or equal
echo $((5 >= 3)); echo $((5 >= 5)); echo $((4 >= 5))
### expect
1
1
0
### end

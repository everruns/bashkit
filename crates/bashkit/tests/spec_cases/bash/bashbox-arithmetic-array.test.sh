# BashBox arithmetic-array cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_arithmetic_array_elements_read_and_write_by_index_nested_and_negative
# elements read and write by index, nested and negative
a=(5 6 7); i=1; echo $((a[i+1] + a[0])) $((a[a[0]-4])) $(( a[-1] )) $(( a[ 1 ] )); ((a[1]+=10, a[5]=3)); echo ${a[@]}; ((a[2]++)); ((--a[0])); echo ${a[0]} ${a[2]} $((a[1]--)) ${a[1]}
### expect
12 6 7 6
5 16 7 3
4 8 16 15
### end

### bashbox_arithmetic_array_associative_keys_stay_text_and_a_scalar_is_its_own_element_0
# associative keys stay text, and a scalar is its own element 0
declare -A m; m[x]=4; echo $((m[x]*2)); ((m[y]=7)); echo ${m[y]}; k=x; echo $((m[$k]+1)); b=3; echo $((b[0])) $((b[1]))
### expect
8
7
5
3 0
### end

### bashbox_arithmetic_array_an_unclosed_subscript_is_an_error
# an unclosed subscript is an error
echo $((a[1 ))
echo next
### expect
next
### end

### bashbox_arithmetic_array_an_empty_subscript_is_reported_twice_and_reads_0
# an empty subscript is reported twice and reads 0
a=(1 2); echo $((a[])) $((a[]+1))
### expect
0 1
### end

### bashbox_arithmetic_array_division_by_0_names_the_divisor
# division by 0 names the divisor
echo $(( 1/0 ))
echo $(( 10 / (2-2) ))
echo $(( 5 % 0 + 1 ))
a=1; (( a /= 0 )); echo $?
echo $(( a %= 0 ))
x="1/0"; echo $(( x + 1 ))
echo $(( 0 && 1/0 ))
### expect
1
0
### end

### bashbox_arithmetic_array_syntax_errors_quote_the_token_bash_stopped_at
# syntax errors quote the token bash stopped at
echo $(( 1 + ))
echo $((1 +))
((1 + )); echo $?
echo $(( 1 2 ))
echo $(( 1 ? 2 ))
echo $(( 1 +* 2 ))
echo $(( 08 ))
let "x = 1 +"; echo $?
### expect
1
1
### end

### bashbox_arithmetic_array_integers_wrap_around_at_64_bits
# integers wrap around at 64 bits
echo $(( 9223372036854775807 + 1 )) $(( 9223372036854775807 * 3 )) $(( -9223372036854775807 - 2 )) $(( 99999999999999999999 )); x=-9223372036854775807; echo $(( (x-1) / -1 )) $(( (x-1) % -1 )) $(( -(x-1) )); y=9223372036854775807; ((y++)); echo $y; ((y*=2)); echo $y
### expect
-9223372036854775808 9223372036854775805 9223372036854775807 7766279631452241919
-9223372036854775808 0 -9223372036854775808
-9223372036854775808
0
### end

### bashbox_arithmetic_array_powers_and_shifts_wrap_like_c
# powers and shifts wrap like C
echo $(( 2 ** 64 )) $(( 3 ** 41 )) $(( 2 ** 63 )) $(( 7 ** 0 )) $(( 1 << 64 )) $(( 1 << 63 )) $(( -1 >> 70 )) $(( 1 << -1 )) $(( 8 >> -62 )); z=1; ((z <<= 65)); echo $z
### expect
0 -420491770248316829 -9223372036854775808 1 1 -9223372036854775808 -1 -9223372036854775808 2
2
### end

### bashbox_arithmetic_array_let_stops_at_a_failing_expression
# let stops at a failing expression
let "1/0" "y=2"; echo $? $y; let; echo $?; let x=1 "y=x+1"; echo $? $y; let 0; echo $?
### expect
1
1
0 2
1
### end

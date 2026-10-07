### brace_not_applied_to_expansion_results
y='{a,b}'
echo $y
for i in $y; do echo "[$i]"; done
z='{1..3}'; echo $z x$z
### expect
{a,b}
[{a,b}]
{1..3} x{1..3}
### end

### brace_range_with_variable_stays_literal
n=3
echo {1..$n}
### expect
{1..3}
### end

### brace_zero_padded_range
echo {01..10..3}
echo {08..11}
echo {-01..1}
echo {1..03}
### expect
01 04 07 10
08 09 10 11
-01 000 001
01 02 03
### end

### brace_quoted_parts
echo "{a,b}"x{1,2}
echo '{a,b}'{c,d}
echo a"{"b,c}
echo {a,"b c"}
echo x{a,b}'y'
### expect
{a,b}x1 {a,b}x2
{a,b}c {a,b}d
a{b,c}
a b c
xay xby
### end

### brace_escaped_comma_and_brace
echo {a,\,}
echo a{b\,c,d}
echo \{a,b\}
### expect
a ,
ab,c ad
{a,b}
### end

### brace_invalid_group_then_valid
echo {x}{a,b}
echo {a{b,c}}
### expect
{x}a {x}b
{ab} {ac}
### end

### brace_with_variable_item
v=V
echo {a,${v}}x pre{1,2}"$v"
### expect
ax Vx pre1V pre2V
### end

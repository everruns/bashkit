### printf_basic
# Basic printf string
printf "hello\n"
### expect
hello
### end

### printf_string_format
# Printf with %s format
printf "%s\n" "world"
### expect
world
### end

### printf_integer
# Printf with %d format
printf "%d\n" 42
### expect
42
### end

### printf_zero_padding
# Printf with zero-padded integer
printf "%05d\n" 42
### expect
00042
### end

### printf_width_space
# Printf with space-padded integer
printf "%5d\n" 42
### expect
   42
### end

### printf_left_align
# Printf with left-aligned integer
printf "%-5d|\n" 42
### expect
42   |
### end

### printf_hex_lower
# Printf hex lowercase
printf "%x\n" 255
### expect
ff
### end

### printf_hex_upper
# Printf hex uppercase
printf "%X\n" 255
### expect
FF
### end

### printf_hex_zero_pad
# Printf hex with zero padding
printf "%04x\n" 255
### expect
00ff
### end

### printf_octal
# Printf octal
printf "%o\n" 8
### expect
10
### end

### printf_float
# Printf float
printf "%.2f\n" 3.14159
### expect
3.14
### end

### printf_string_width
# Printf string with width
printf "%5s|\n" "hi"
### expect
   hi|
### end

### printf_string_left
# Printf string left-aligned
printf "%-5s|\n" "hi"
### expect
hi   |
### end

### printf_escape_n
# Printf newline escape
printf "a\nb\n"
### expect
a
b
### end

### printf_escape_t
# Printf tab escape
printf "a\tb\n"
### expect
a	b
### end

### printf_percent
# Printf literal percent
printf "100%%\n"
### expect
100%
### end

### printf_multiple_args
# Printf with multiple arguments
printf "%s is %d years old\n" "Alice" 30
### expect
Alice is 30 years old
### end

### printf_negative_zero_pad
# Printf zero-padded negative integer
printf "%06d\n" -42
### expect
-00042
### end

### printf_array_expansion
# Printf with array expansion - format string repeats per element
colors=(Black Red Green Yellow Blue Magenta Cyan White)
printf "%s\n" "${colors[@]}"
### expect
Black
Red
Green
Yellow
Blue
Magenta
Cyan
White
### end

### printf_array_at_format_reuse
# Printf reuses format for each array element via ${arr[@]}
nums=(1 2 3)
printf "%d\n" "${nums[@]}"
### expect
1
2
3
### end

### printf_array_star_quoted
# "${arr[*]}" joins elements into single arg
arr=(a b c)
printf "[%s]\n" "${arr[*]}"
### expect
[a b c]
### end

### printf_array_at_with_format
# Printf with multi-specifier format and array args
items=(Alice 30 Bob 25)
printf "%s is %d\n" "${items[@]}"
### expect
Alice is 30
Bob is 25
### end

### printf_empty_array
# Printf with empty array - format runs once with empty %s
arr=()
result=$(printf "%s\n" "${arr[@]}")
echo "[$result]"
### expect
[]
### end

### printf_single_element_array
# Printf with single-element array
arr=(only)
printf "(%s)\n" "${arr[@]}"
### expect
(only)
### end

### printf_v_flag
# printf -v assigns to variable
printf -v result "%d + %d = %d" 3 4 7; echo "$result"
### expect
3 + 4 = 7
### end

### printf_v_formatted
# printf -v with padding
printf -v padded "%05d" 42; echo "$padded"
### expect
00042
### end

### printf_q_space
# printf %q escapes spaces
printf '%q\n' 'hello world'
### expect
hello\ world
### end

### printf_q_simple
# printf %q leaves safe strings unquoted
printf '%q\n' 'simple'
### expect
simple
### end

### printf_q_empty
# printf %q quotes empty string
printf '%q\n' ''
### expect
''
### end

### printf_q_special_chars
# printf %q escapes special shell chars
printf '%q\n' 'a"b'
### expect
a\"b
### end

### printf_q_tab
# printf %q uses $'...' for control chars
### bash_diff
x=$(printf 'hello\tworld')
printf '%q\n' "$x"
### expect
$'hello\tworld'
### end

### printf_q_single_quote
# printf %q escapes single quotes
printf '%q\n' "it's"
### expect
it\'s
### end

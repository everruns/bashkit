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

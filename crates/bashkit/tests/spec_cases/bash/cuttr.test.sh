### cut_single_field
# Extract single field
printf 'a,b,c\n1,2,3\n' | cut -d, -f2
### expect
b
2
### end

### cut_multiple_fields
# Extract multiple fields
printf 'a,b,c\n1,2,3\n' | cut -d, -f1,3
### expect
a,c
1,3
### end

### cut_field_range
# Extract field range
printf 'a:b:c:d\n' | cut -d: -f1-2
### expect
a:b
### end

### tr_lowercase_to_uppercase
# Translate lowercase to uppercase
printf 'hello\n' | tr a-z A-Z
### expect
HELLO
### end

### tr_delete
# Delete characters
printf 'hello world\n' | tr -d aeiou
### expect
hll wrld
### end

### tr_single_char
# Translate single character
printf 'a:b:c\n' | tr : -
### expect
a-b-c
### end

### tr_spaces_to_newlines
# Replace spaces with newlines
printf 'one two three\n' | tr ' ' '\n'
### expect
one
two
three
### end

### cut_no_field
# Cut without field specification should error
printf 'a,b,c\n' | cut -d, 2>/dev/null
echo $?
### expect
1
### end

### cut_empty_input
# Cut with empty input
printf '' | cut -d, -f1
echo done
### expect
done
### end

### tr_delete_all_vowels
# Delete all vowels
printf 'HELLO WORLD\n' | tr -d AEIOU
### expect
HLL WRLD
### end

### cut_char_range
# Cut character range
printf 'hello world\n' | cut -c1-5
### expect
hello
### end

### cut_char_single
# Cut single character
printf 'hello\n' | cut -c1
### expect
h
### end

### cut_char_multiple
# Cut multiple chars
printf 'hello\n' | cut -c1,3,5
### expect
hlo
### end

### cut_char_from_end
# Cut from start to position N
printf 'hello\n' | cut -c-3
### expect
hel
### end

### cut_char_to_end
# Cut from position to end
printf 'hello world\n' | cut -c7-
### expect
world
### end

### cut_field_from_end
# Cut fields from start
printf 'a:b:c:d:e\n' | cut -d: -f-3
### expect
a:b:c
### end

### cut_field_to_end
# Cut fields to end
printf 'a:b:c:d:e\n' | cut -d: -f3-
### expect
c:d:e
### end

### cut_complement
# Complement field selection
printf 'a,b,c,d\n' | cut -d, --complement -f2
### expect
a,c,d
### end

### cut_output_delimiter
# Custom output delimiter
printf 'a,b,c\n' | cut -d, -f1,3 --output-delimiter=-
### expect
a-c
### end

### cut_tab_default
# Default tab delimiter
printf 'a\tb\tc\n' | cut -f2
### expect
b
### end

### tr_squeeze
# Squeeze repeated characters
printf 'heeelllo   wooorld\n' | tr -s 'eol '
### expect
helo world
### end

### tr_complement
# Complement character set — delete all non-digits
printf 'hello123\n' | tr -cd '0-9\n'
### expect
123
### end

### tr_class_lower
# Character class [:lower:]
printf 'Hello World\n' | tr '[:upper:]' '[:lower:]'
### expect
hello world
### end

### tr_class_upper
# Character class [:upper:]
printf 'Hello World\n' | tr '[:lower:]' '[:upper:]'
### expect
HELLO WORLD
### end

### tr_class_digit
# Delete digits using character class
printf 'a1b2c3\n' | tr -d '[:digit:]'
### expect
abc
### end

### tr_class_alpha
# Delete alpha using character class
printf 'a1b2c3\n' | tr -d '[:alpha:]'
### expect
123
### end

### tr_escape_newline
# Translate to newline
printf 'a:b:c\n' | tr ':' '\n'
### expect
a
b
c
### end

### tr_escape_tab
# Translate to tab
printf 'a b c\n' | tr ' ' '\t'
### expect
a	b	c
### end

### tr_multiple_chars
# Translate multiple chars
printf 'aabbcc\n' | tr 'abc' 'xyz'
### expect
xxyyzz
### end

### tr_truncate_set2
# When SET2 shorter, last char repeats
printf 'aabbcc\n' | tr 'abc' 'x'
### expect
xxxxxx
### end

### cut_only_delimited
# Only print lines containing delimiter
printf 'a,b,c\nno delim\nx,y\n' | cut -d, -f1 -s
### expect
a
x
### end

### cut_zero_terminated
printf 'a,b\0x,y\0' | cut -d, -f2 -z | tr '\0' '\n'
### expect
b
y
### end

### cut_byte_mode
# -b is alias for -c
printf 'hello world\n' | cut -b1-5
### expect
hello
### end

### tr_complement_uppercase_C
# -C is POSIX alias for -c (complement)
printf 'hello123\n' | tr -Cd '0-9\n'
### expect
123
### end

### tr_delete_punct
# Delete punctuation using [:punct:]
printf 'hello, world!\n' | tr -d '[:punct:]'
### expect
hello world
### end

### tr_class_alnum
# Delete non-alnum chars using complement
printf 'a1!b2@c3#\n' | tr -cd '[:alnum:]\n'
### expect
a1b2c3
### end

### tr_class_space
# Squeeze spaces using [:space:]
printf 'hello   world\n' | tr -s '[:space:]'
### expect
hello world
### end

### tr_squeeze_translate
# Translate and squeeze
printf 'aabbcc\n' | tr -s 'a-z' 'A-Z'
### expect
ABC
### end

### tr_class_blank
# Delete blanks using [:blank:]
printf 'a\tb c\n' | tr -d '[:blank:]'
### expect
abc
### end

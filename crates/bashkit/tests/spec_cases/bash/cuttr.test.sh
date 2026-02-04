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
### skip: test expects trailing newline but tr preserves input format
# Translate lowercase to uppercase
printf 'hello' | tr a-z A-Z
### expect
HELLO
### end

### tr_delete
### skip: test expects trailing newline but tr preserves input format
# Delete characters
printf 'hello world' | tr -d aeiou
### expect
hll wrld
### end

### tr_single_char
### skip: test expects trailing newline but tr preserves input format
# Translate single character
printf 'a:b:c' | tr : -
### expect
a-b-c
### end

### tr_spaces_to_newlines
### skip: tr escape sequence processing not implemented
# Replace spaces with newlines
printf 'one two three' | tr ' ' '\n'
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
### skip: test expects trailing newline but tr preserves input format
# Delete all vowels
printf 'HELLO WORLD' | tr -d AEIOU
### expect
HLL WRLD
### end

### cut_char_range
### skip: cut -c (character mode) not implemented
# Cut character range
printf 'hello world\n' | cut -c1-5
### expect
hello
### end

### cut_char_single
### skip: cut -c (character mode) not implemented
# Cut single character
printf 'hello\n' | cut -c1
### expect
h
### end

### cut_char_multiple
### skip: cut -c (character mode) not implemented
# Cut multiple chars
printf 'hello\n' | cut -c1,3,5
### expect
hlo
### end

### cut_char_from_end
### skip: cut -c (character mode) not implemented
printf 'hello\n' | cut -c-3
### expect
hel
### end

### cut_char_to_end
### skip: cut -c (character mode) not implemented
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
### skip: cut f3- (field to end) syntax not implemented
# Cut fields to end
printf 'a:b:c:d:e\n' | cut -d: -f3-
### expect
c:d:e
### end

### cut_complement
### skip: cut --complement not implemented
printf 'a,b,c,d\n' | cut -d, --complement -f2
### expect
a,c,d
### end

### cut_output_delimiter
### skip: cut --output-delimiter not implemented
printf 'a,b,c\n' | cut -d, -f1,3 --output-delimiter='-'
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
### skip: tr -s (squeeze) not implemented
# Squeeze repeated characters
printf 'heeelllo   wooorld' | tr -s 'eol '
### expect
helo world
### end

### tr_complement
### skip: tr -c (complement) not implemented
# Complement character set
printf 'hello123' | tr -cd '0-9'
### expect
123
### end

### tr_class_lower
### skip: tr character classes not implemented
# Character class [:lower:]
printf 'Hello World' | tr '[:upper:]' '[:lower:]'
### expect
hello world
### end

### tr_class_upper
### skip: tr character classes not implemented
# Character class [:upper:]
printf 'Hello World' | tr '[:lower:]' '[:upper:]'
### expect
HELLO WORLD
### end

### tr_class_digit
### skip: tr character classes not implemented
printf 'a1b2c3' | tr -d '[:digit:]'
### expect
abc
### end

### tr_class_alpha
### skip: tr character classes not implemented
printf 'a1b2c3' | tr -d '[:alpha:]'
### expect
123
### end

### tr_escape_newline
### skip: tr escape sequence processing not implemented
# Translate to newline
printf 'a:b:c' | tr ':' '\n'
### expect
a
b
c
### end

### tr_escape_tab
### skip: tr escape sequence processing not implemented
# Translate to tab
printf 'a b c' | tr ' ' '\t'
### expect
a	b	c
### end

### tr_multiple_chars
### skip: test expects trailing newline but tr preserves input format
# Translate multiple chars
printf 'aabbcc' | tr 'abc' 'xyz'
### expect
xxyyzz
### end

### tr_truncate_set2
### skip: test expects trailing newline but tr preserves input format
printf 'aabbcc' | tr 'abc' 'x'
### expect
xxxxxx
### end

### cut_only_delimited
### skip: cut -s (only delimited) not implemented
printf 'a,b,c\nno delim\nx,y\n' | cut -d, -f1 -s
### expect
a
x
### end

### cut_zero_terminated
### skip: cut -z (zero-terminated) not implemented
printf 'a,b\0x,y\0' | cut -d, -f2 -z | tr '\0' '\n'
### expect
b
y
### end

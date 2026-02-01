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
printf 'hello' | tr a-z A-Z
### expect
HELLO
### end

### tr_delete
# Delete characters
printf 'hello world' | tr -d aeiou
### expect
hll wrld
### end

### tr_single_char
# Translate single character
printf 'a:b:c' | tr : -
### expect
a-b-c
### end

### tr_spaces_to_newlines
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
# Delete all vowels
printf 'HELLO WORLD' | tr -d AEIOU
### expect
HLL WRLD
### end

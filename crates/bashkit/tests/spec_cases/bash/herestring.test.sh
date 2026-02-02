### herestring_cat
# Basic here string with cat
cat <<< "hello world"
### expect
hello world
### end

### herestring_variable
# Here string with variable expansion
name="world"
cat <<< "hello $name"
### expect
hello world
### end

### herestring_multiword
# Here string with multiple words
cat <<< "one two three"
### expect
one two three
### end

### herestring_read
# Here string with read command
read var <<< "input value"
echo "$var"
### expect
input value
### end

### herestring_empty
### skip: empty herestring adds extra newline
# Empty here string
cat <<< ""
### expect

### end

### herestring_with_variable
# Here string with variable expansion
msg="universe"
cat <<< "hello $msg"
### expect
hello universe
### end

### herestring_single_quotes
# Here string with single quotes (literal)
cat <<< 'hello $var world'
### expect
hello $var world
### end

### herestring_numbers
# Here string with numbers
cat <<< "123 456"
### expect
123 456
### end

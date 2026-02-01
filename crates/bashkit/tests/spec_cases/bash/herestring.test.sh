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
# Empty here string
cat <<< ""
### expect

### end

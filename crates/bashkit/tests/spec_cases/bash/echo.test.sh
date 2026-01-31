### echo_simple
# Basic echo command
echo hello
### expect
hello
### end

### echo_multiple_words
# Echo with multiple arguments
echo hello world
### expect
hello world
### end

### echo_empty
# Echo with no arguments
echo
### expect

### end

### echo_quoted_string
# Echo with double quotes
echo "hello world"
### expect
hello world
### end

### echo_single_quoted
# Echo with single quotes
echo 'hello world'
### expect
hello world
### end

### echo_escape_n
# Echo with -e and newline
echo -e "hello\nworld"
### expect
hello
world
### end

### echo_escape_t
# Echo with -e and tab
echo -e "hello\tworld"
### expect
hello	world
### end

### echo_no_newline
# Echo with -n flag
printf '%s' "$(echo -n hello)"
### expect
hello
### end

### echo_mixed_quotes
# Mixed quoting
echo "hello" 'world'
### expect
hello world
### end

### echo_preserves_spaces
# Spaces in quotes preserved
echo "hello   world"
### expect
hello   world
### end

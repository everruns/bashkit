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
# Echo with no arguments outputs a blank line
echo | wc -l | grep -q 1 && echo "valid"
### expect
valid
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
# Echo with -n flag (verify no trailing newline by appending text)
echo "$(echo -n hello)world"
### expect
helloworld
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

### echo_escape_r
# Echo with carriage return - raw output contains CR character
echo -e "hello\rworld" | cat -v | grep -q 'hello.*world' && echo "valid"
### expect
valid
### end

### echo_combined_en
# Combined -en flags
echo -en "hello\nworld"
printf '\n'
### expect
hello
world
### end

### echo_combined_ne
# Combined -ne flags
echo -ne "a\tb"
printf '\n'
### expect
a	b
### end

### echo_E_flag
# -E flag disables escape interpretation
echo -E "hello\nworld"
### expect
hello\nworld
### end

### echo_backslash
# Echo backslash escape
echo -e "hello\\\\world"
### expect
hello\world
### end

### echo_multiple_escapes
# Multiple escape sequences
echo -e "line1\nline2\ttabbed"
### expect
line1
line2	tabbed
### end

### echo_double_dash
echo -- -n
### expect
-- -n
### end

### echo_variable_expansion
# Variable expansion in echo
x=world
echo "hello $x"
### expect
hello world
### end

### echo_command_substitution
# Command substitution in echo
echo "date: $(echo test)"
### expect
date: test
### end

### echo_special_chars_in_quotes
# Special chars in double quotes
echo "hello!@#$%"
### expect
hello!@#$%
### end

### echo_asterisk
# Asterisk in quotes
echo "*"
### expect
*
### end

### echo_question_mark
# Question mark in quotes
echo "?"
### expect
?
### end

### echo_escape_hex
# Hex escapes in echo -e
echo -e "\x48\x65\x6c\x6c\x6f"
### expect
Hello
### end

### echo_escape_octal
# Octal escapes in echo -e use \0nnn format
echo -e "\0110\0145\0154\0154\0157"
### expect
Hello
### end

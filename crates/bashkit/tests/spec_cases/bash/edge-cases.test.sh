### empty_script
# Empty script should succeed

### expect
### end

### whitespace_only
# Whitespace-only script should succeed

### expect
### end

### comment_only
# Comment-only script should succeed
# This is a comment
### expect
### end

### very_long_line
# Long output line handling
echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
### expect
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
### end

### unicode_in_strings
# Unicode characters in strings
echo "Hello, 世界! 🚀"
### expect
Hello, 世界! 🚀
### end

### unicode_in_variable
# Unicode in variable value
### skip: variable assignment on same line issue
MSG="Hello World"; echo "$MSG"
### expect
Hello World
### end

### special_chars_in_string
# Special characters in quoted string
echo "tab:	newline:
done"
### expect
tab:	newline:
done
### end

### backslash_in_string
# Backslash handling in double quotes
echo "path\\to\\file"
### expect
path\to\file
### end

### dollar_in_single_quote
# Dollar sign literal in single quotes
echo '$HOME'
### expect
$HOME
### end

### nested_quotes
# Nested quote handling
echo "She said 'hello'"
### expect
She said 'hello'
### end

### empty_variable_expansion
# Empty variable expands to nothing
EMPTY=""
echo "before${EMPTY}after"
### expect
beforeafter
### end

### multiple_semicolons
# Multiple semicolons are valid
echo a;; echo b
### skip: multiple semicolons parsing not supported
### expect
a
b
### end

### trailing_semicolon
# Trailing semicolon is valid
echo hello;
### expect
hello
### end

### leading_semicolon
# Leading semicolon is valid (no-op)
### skip: leading semicolon parsing
; echo hello
### expect
hello
### end

### many_nested_subshells
# Deeply nested subshells
echo $(echo $(echo $(echo $(echo hello))))
### expect
hello
### end

### pipeline_exit_code
# Pipeline exit code is last command
true | false
echo $?
### expect
1
### end

### and_list_short_circuit
# AND list short circuits on failure
false && echo "should not print"
echo "after"
### expect
after
### end

### or_list_short_circuit
# OR list short circuits on success
true || echo "should not print"
echo "after"
### expect
after
### end

### mixed_and_or
# Mixed AND/OR list
false || true && echo "yes"
### expect
yes
### end

### variable_in_variable_name
# Indirect variable reference not supported
### skip: indirect references not implemented
FOO=BAR
BAR=value
echo ${!FOO}
### expect
value
### end

### arithmetic_with_spaces
# Spaces in arithmetic expression
echo $((  1  +  2  ))
### expect
3
### end

### negative_arithmetic
# Negative numbers in arithmetic
echo $((-5 + 3))
### expect
-2
### end

### zero_exit_code
# Zero is truthy in shell
if [ 0 -eq 0 ]; then echo yes; fi
### expect
yes
### end

### empty_if_body
# Empty if body is valid
### skip: empty body parsing
if true; then
fi
echo done
### expect
done
### end

### case_default
# Case default pattern
X=unknown
case $X in
  foo) echo foo;;
  *) echo default;;
esac
### expect
default
### end

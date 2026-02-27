### subst_simple
# Simple command substitution
echo $(echo hello)
### expect
hello
### end

### subst_in_string
# Command substitution in string
echo "result: $(echo 42)"
### expect
result: 42
### end

### subst_pipeline
# Command substitution with pipeline
echo $(echo hello | cat)
### expect
hello
### end

### subst_assign
# Assign command substitution to variable
VAR=$(echo test); echo $VAR
### expect
test
### end

### subst_nested
# Nested command substitution
echo $(echo $(echo deep))
### expect
deep
### end

### subst_multiline
# Multi-line output
echo "$(printf 'a\nb\nc')"
### expect
a
b
c
### end

### subst_with_args
# Command with arguments
echo $(printf '%s %s' hello world)
### expect
hello world
### end

### subst_arithmetic
# Command in arithmetic context
X=$(echo 5); echo $((X + 3))
### expect
8
### end

### subst_in_condition
# Command substitution in condition
if [ "$(echo yes)" = "yes" ]; then echo matched; fi
### expect
matched
### end

### subst_exit_code
# Exit code from command substitution
result=$(false); echo $?
### expect
1
### end

### subst_backtick
echo `echo hello`
### expect
hello
### end

### subst_multiple
# Multiple substitutions
echo $(echo a) $(echo b) $(echo c)
### expect
a b c
### end

### subst_with_variable
# Substitution using variable
NAME=test; echo $(echo $NAME)
### expect
test
### end

### subst_strip_trailing_newlines
# Command substitution strips trailing newlines
VAR=$(printf 'hello\n\n\n'); echo "x${VAR}y"
### expect
xhelloy
### end

### subst_nested_quotes
# Nested double quotes inside $() inside double quotes
echo "$(echo "hello world")"
### expect
hello world
### end

### subst_nested_quotes_var
# Variable expansion in nested quoted $()
x="John"; echo "Hello, $(echo "$x")!"
### expect
Hello, John!
### end

### subst_deeply_nested_quotes
# Deeply nested $() with quotes
echo "nested: $(echo "$(echo "deep")")"
### expect
nested: deep
### end

### subst_nested_single_quotes
# Single quotes inside $() inside double quotes
echo "$(echo 'single quoted')"
### expect
single quoted
### end

### subst_nested_quotes_no_expand
# Nested quotes without variable (literal string)
echo "result=$(echo "done")"
### expect
result=done
### end

### subst_nested_quotes_empty
# Nested quotes with empty inner string
echo "x$(echo "")y"
### expect
xy
### end

### subst_nested_quotes_multiple
# Multiple nested $() in same double-quoted string
echo "$(echo "a") and $(echo "b")"
### expect
a and b
### end

### subst_nested_quotes_escape
# Escaped characters inside nested $()
echo "$(echo "hello\"world")"
### expect
hello"world
### end

### subst_word_split_for_loop
# Command substitution output is word-split in for-loop list context
mkdir -p /src
echo "a" > /src/one.txt
echo "b" > /src/two.txt
echo "c" > /src/three.txt
count=0
for f in $(find /src -name "*.txt" -type f | sort); do
  count=$((count + 1))
done
echo "$count"
### expect
3
### end

### subst_word_split_echo_multiword
# Command substitution producing space-separated words splits in for-loop
result=""
for w in $(echo "alpha beta gamma"); do
  result="${result}[${w}]"
done
echo "$result"
### expect
[alpha][beta][gamma]
### end

### subst_word_split_newlines
# Command substitution with newline-separated output splits on newlines
result=""
for line in $(printf 'x\ny\nz'); do
  result="${result}(${line})"
done
echo "$result"
### expect
(x)(y)(z)
### end

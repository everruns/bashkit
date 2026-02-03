### pipe_simple
# Simple pipe
echo hello | cat
### expect
hello
### end

### pipe_chain
# Pipe chain
echo hello | cat | cat
### expect
hello
### end

### pipe_grep
# Pipe to grep
printf "foo\nbar\nbaz\n" | grep bar
### expect
bar
### end

### pipe_multiple_lines
# Pipe with multiple lines
printf "a\nb\nc\n" | cat
### expect
a
b
c
### end

### redirect_out
# Redirect stdout to file
echo hello > /tmp/test.txt; cat /tmp/test.txt
### expect
hello
### end

### redirect_append
# Redirect append
echo hello > /tmp/append.txt; echo world >> /tmp/append.txt; cat /tmp/append.txt
### expect
hello
world
### end

### redirect_in
# Redirect input from file
echo content > /tmp/input.txt; cat < /tmp/input.txt
### expect
content
### end

### here_string
# Here string
cat <<< hello
### expect
hello
### end

### heredoc_simple
# Simple heredoc
cat <<EOF
hello
world
EOF
### expect
hello
world
### end

### heredoc_single_line
# Single line heredoc
cat <<END
test
END
### expect
test
### end

### heredoc_with_vars
# Heredoc with variable expansion
NAME=world; cat <<EOF
hello $NAME
EOF
### expect
hello world
### end

### redirect_stderr_to_file
# Redirect stderr to file
echo error 2>/tmp/err.txt; cat /tmp/err.txt
### expect
error
### end

### redirect_stderr_with_dup
# Redirect stderr to stdout (2>&1)
echo "hello" > /tmp/combined.txt 2>&1; cat /tmp/combined.txt
### expect
hello
### end

### redirect_both_ampersand
# Redirect both with &>
echo "output" &> /tmp/both.txt; cat /tmp/both.txt
### expect
output
### end

### redirect_fd2_append
# Append stderr to file (2>>)
echo err1 2>/tmp/err_append.txt; echo err2 2>>/tmp/err_append.txt; cat /tmp/err_append.txt
### expect
err1
err2
### end

### heredoc_single_quoted_delimiter
# Heredoc with single-quoted delimiter disables variable expansion
NAME=world; cat <<'EOF'
hello $NAME
EOF
### expect
hello $NAME
### end

### heredoc_double_quoted_delimiter
# Heredoc with double-quoted delimiter also disables expansion
NAME=world; cat <<"EOF"
hello $NAME
EOF
### expect
hello $NAME
### end

### heredoc_quoted_with_special_chars
# Single-quoted heredoc preserves special characters
cat <<'PY'
price = 100
print(f"${price}")
PY
### expect
price = 100
print(f"${price}")
### end

### heredoc_unquoted_expands
# Unquoted delimiter allows variable expansion (control test)
VAR=expanded; cat <<END
value is $VAR
END
### expect
value is expanded
### end

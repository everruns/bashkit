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

### redirect_stderr
# Redirect stdout to stderr
echo error >&2
### expect
### end

### redirect_both
# Redirect stdout to file with stderr also going there
echo hello > /tmp/out.txt 2>&1
### expect
### end

### heredoc_basic
cat <<EOF
hello world
EOF
### expect
hello world
### end

### heredoc_variable
NAME=world
cat <<EOF
hello $NAME
EOF
### expect
hello world
### end

### heredoc_braced_variable
NAME=world
cat <<EOF
hello ${NAME}!
EOF
### expect
hello world!
### end

### heredoc_command_subst
cat <<EOF
$(echo hello from cmd)
EOF
### expect
hello from cmd
### end

### heredoc_arithmetic
cat <<EOF
result is $((2 + 3))
EOF
### expect
result is 5
### end

### heredoc_quoted_delimiter
NAME=world
cat <<'EOF'
hello $NAME
EOF
### expect
hello $NAME
### end

### heredoc_multiline
A=foo
B=bar
cat <<EOF
first: $A
second: $B
third: literal
EOF
### expect
first: foo
second: bar
third: literal
### end

### heredoc_to_file
cat > /tmp/heredoc_out.txt <<EOF
line one
line two
EOF
cat /tmp/heredoc_out.txt
### expect
line one
line two
### end

### heredoc_mixed_expansion
X=42
cat <<EOF
value: $X, cmd: $(echo hi), math: $((X * 2))
EOF
### expect
value: 42, cmd: hi, math: 84
### end

### heredoc_tab_strip
# <<- strips leading tabs from content and delimiter
cat <<-EOF
	hello
	world
	EOF
### expect
hello
world
### end

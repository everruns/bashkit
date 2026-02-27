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

### heredoc_redirect_after
# cat <<EOF > file should write heredoc content to file, not stdout
cat <<EOF > /tmp/heredoc_redirect.txt
line one
line two
EOF
cat /tmp/heredoc_redirect.txt
### expect
line one
line two
### end

### heredoc_redirect_after_with_vars
# cat <<EOF > file with variable expansion
NAME=world
cat <<EOF > /tmp/heredoc_vars.txt
hello $NAME
EOF
cat /tmp/heredoc_vars.txt
### expect
hello world
### end

### heredoc_redirect_after_multiline
# cat <<EOF > file with multiline YAML-like content (issue #345)
mkdir -p /etc/app
cat <<EOF > /etc/app/config.yaml
app:
  name: myservice
  port: 8080
EOF
cat /etc/app/config.yaml
### expect
app:
  name: myservice
  port: 8080
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

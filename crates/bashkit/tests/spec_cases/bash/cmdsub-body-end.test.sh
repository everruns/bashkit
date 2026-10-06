# The `)` that ends $(...) is found the way bash finds it: parens inside
# quotes, escapes, comments and heredoc bodies do not close the substitution.

### cmdsub_heredoc_with_paren
x=$(cat <<'E'
hello )
E
)
echo "$x"
### expect
hello )
### end

### cmdsub_heredoc_in_double_quotes
msg="$(cat <<'EOF'
fix: thing (scope)

body )
EOF
)"
echo "$msg"
### expect
fix: thing (scope)

body )
### end

### cmdsub_dash_heredoc
x=$(cat <<-EOF
	a ( b
	EOF
)
echo "$x"
### expect
a ( b
### end

### cmdsub_quoted_paren
x=$(echo ")"); echo "$x"
y="$(echo ")")"; echo "$y"
z=$(echo 'a)'); echo "$z"
### expect
)
)
a)
### end

### cmdsub_escaped_paren
x=$(echo \)); echo "$x"
### expect
)
### end

### cmdsub_comment_with_paren
x=$(echo a # c)
)
echo "$x"
### expect
a
### end

### cmdsub_arith_shift_not_heredoc
echo $(echo $((1<<3)))
### expect
8
### end

### cmdsub_here_string
x=$(cat <<< "hi)"); echo "$x"
### expect
hi)
### end

### syntax_error_runs_earlier_lines
bash -c 'echo first
echo second
if then
echo never' 2>/dev/null
echo "rc=$?"
### expect
first
second
rc=2
### end

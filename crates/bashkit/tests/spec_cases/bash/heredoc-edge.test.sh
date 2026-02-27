# Heredoc edge cases
# Inspired by Oils spec/here-doc.test.sh
# https://github.com/oilshell/oil/blob/master/spec/here-doc.test.sh

### heredoc_with_var_sub
# Here doc with var sub, command sub, arith sub
var=v
cat <<EOF
var: ${var}
command: $(echo hi)
arith: $((1+2))
EOF
### expect
var: v
command: hi
arith: 3
### end

### heredoc_quoted_delimiter
# Quoted delimiter prevents expansion
var=should_not_expand
cat <<'EOF'
$var $(echo nope) $((1+2))
EOF
### expect
$var $(echo nope) $((1+2))
### end

### heredoc_partial_quote_delimiter
# Partial quote in delimiter still prevents expansion
### skip: TODO partial quoting in heredoc delimiter not implemented
cat <<'EOF'"2"
one
two
EOF2
### expect
one
two
### end

### heredoc_pipe_first_line
# Here doc with pipe on first line
### skip: TODO heredoc piped to sort - pipe ordering issue
cat <<EOF | sort
c
a
b
EOF
### expect
a
b
c
### end

### heredoc_pipe_last_line
# Here doc with pipe continued on last line
### skip: TODO heredoc pipe continuation - pipe ordering issue
cat <<EOF |
c
a
b
EOF
sort
### expect
a
b
c
### end

### heredoc_with_read
# Here doc with builtin 'read'
read v1 v2 <<EOF
val1 val2
EOF
echo =$v1= =$v2=
### expect
=val1= =val2=
### end

### heredoc_compound_while
# Compound command here doc with while
while read line; do
  echo X $line
done <<EOF
1
2
3
EOF
### expect
X 1
X 2
X 3
### end

### heredoc_in_while_condition
# Here doc in while condition and body
### skip: TODO multiple heredocs in while condition not parsed
while cat <<E1 && cat <<E2; do cat <<E3; break; done
1
E1
2
E2
3
E3
### expect
1
2
3
### end

### heredoc_multiline_condition
# Here doc in while condition on multiple lines
### skip: TODO multiple heredocs in while condition not parsed
while cat <<E1 && cat <<E2
1
E1
2
E2
do
  cat <<E3
3
E3
  break
done
### expect
1
2
3
### end

### heredoc_with_multiline_dquote
# Here doc with multiline double quoted string
### skip: TODO heredoc followed by multiline dquote on same line not parsed correctly
cat <<EOF; echo "two
three"
one
EOF
### expect
one
two
three
### end

### heredoc_tab_strip
# Here doc with tab stripping (<<-)
cat <<-EOF
	indented
	also indented
EOF
### expect
indented
also indented
### end

### heredoc_empty
# Empty here doc
cat <<EOF
EOF
echo done
### expect
done
### end

### heredoc_single_line
# Single line here doc
cat <<EOF
hello
EOF
### expect
hello
### end

### heredoc_with_blank_lines
# Here doc preserves blank lines
cat <<EOF
a

b

c
EOF
### expect
a

b

c
### end

### heredoc_nested_command_sub
# Here doc inside command substitution
result=$(cat <<EOF
inside command sub
EOF
)
echo "$result"
### expect
inside command sub
### end

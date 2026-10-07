### eval_syntax_error_status
eval "if then" 2>/dev/null
echo "rc=$?"
eval 'echo (' 2>/dev/null
echo "rc=$?"
### expect
rc=2
rc=2
### end

### eval_syntax_error_redirected
x=$(eval "fi" 2>&1)
echo "rc=$? lines=$(printf '%s\n' "$x" | grep -c 'eval: line 1: syntax error')"
### expect
rc=2 lines=1
### end

### eval_syntax_error_line_and_newline
echo start
eval "fi" 2>&1 | grep -c '^bash: eval: line 2: syntax error.*[^ ]$'
echo done
### expect
start
1
done
### end

### lineno_inside_command_substitution
echo one >/dev/null
x=$(echo $LINENO)
y=$(
echo $LINENO)
echo "$x $y"
### expect
2 4
### end

# A builtin's usage error (bad regex, awk syntax error, missing operand)
# fails that one command with exit 2; the script keeps running.

### grep_invalid_regex_continues
echo a | grep -E "(" 2>/dev/null; echo "after=$?"
echo a | grep -E "(" 2>&1 | grep -c '^grep: '
### expect
after=2
1
### end

### grep_missing_pattern_continues
grep 2>/dev/null; echo "after=$?"
### expect
after=2
### end

### grep_missing_pattern_file_continues
grep -f /tmp/no-such-patterns a 2>&1; echo "after=$?"
### expect
grep: /tmp/no-such-patterns: No such file or directory
after=2
### end

### awk_syntax_error_continues
# Exit status (2 mawk, 1 gawk) and message layout vary by awk; only check
# that the command fails, reports on stderr, and the script continues.
awk 'BEGIN{,}' 2>/dev/null || echo failed
awk 'BEGIN{,}' 2>&1 >/dev/null | grep -q 'awk' && echo reported
echo after
### expect
failed
reported
after
### end

### rg_invalid_regex_continues
### bash_diff: rg is not installed on the reference host
echo a | rg "(" 2>/dev/null; echo "after=$?"
### expect
after=2
### end

### usage_error_in_pipeline_and_errexit
echo a | grep -E "(" 2>/dev/null | cat; echo "pipe=$?"
set -e
(echo a | grep -E "(" 2>/dev/null) || echo "caught=$?"
echo done
### expect
pipe=0
caught=2
done
### end

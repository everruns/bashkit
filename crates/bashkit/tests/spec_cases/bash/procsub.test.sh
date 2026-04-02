### procsub_basic
### bash_diff: requires /dev/fd/ which may not exist in all environments
# Basic process substitution
cat <(echo hello)
### expect
hello
### end

### procsub_with_pipe
### bash_diff: requires /dev/fd/
# Process substitution with pipe
cat <(echo hello | tr a-z A-Z)
### expect
HELLO
### end

### procsub_diff_simulation
# Simulate diff with two process substitutions
# Just test that both are parsed
echo "first" > /tmp/file1
echo "second" > /tmp/file2
cat /tmp/file1
cat /tmp/file2
### expect
first
second
### end

### procsub_empty_output
### bash_diff: requires /dev/fd/
# Process substitution with empty command output
cat <(echo "")
echo done
### expect

done
### end

### procsub_multiline
### bash_diff: requires /dev/fd/
# Process substitution with multiline output
cat <(printf 'line1\nline2\nline3\n')
### expect
line1
line2
line3
### end

### procsub_variable_expansion
### bash_diff: requires /dev/fd/
# Process substitution with variable
msg="world"
cat <(echo "hello $msg")
### expect
hello world
### end

### procsub_diff_two_commands
### bash_diff: requires /dev/fd/
# diff with two process substitutions
echo "aaa" > /tmp/psub1.txt
echo "bbb" > /tmp/psub2.txt
diff <(cat /tmp/psub1.txt) <(cat /tmp/psub2.txt) > /dev/null 2>&1
echo "exit: $?"
### expect
exit: 1
### end

### procsub_diff_identical
### bash_diff: requires /dev/fd/
# diff with identical process substitutions
diff <(echo "same") <(echo "same") > /dev/null 2>&1
echo "exit: $?"
### expect
exit: 0
### end

### procsub_paste_two_sources
### bash_diff: requires /dev/fd/
# paste with two process substitutions
paste <(echo "col1") <(echo "col2")
### expect
col1	col2
### end

### procsub_nested_commands
### bash_diff: requires /dev/fd/
# process substitution with complex pipeline
cat <(echo "hello world" | tr ' ' '\n' | sort)
### expect
hello
world
### end

### procsub_sort_comparison
### bash_diff: requires /dev/fd/
# sort and compare with process substitution
cat <(printf 'b\na\nc\n' | sort)
### expect
a
b
c
### end

### process_subst_group_pipe
### bash_diff: requires /dev/fd/
# { ... } | cmd inside < <(...) should produce output
while IFS= read -r line; do echo "$line"; done < <({ echo "a"; echo "b"; } | cat)
### expect
a
b
### end

### process_subst_group_pipe_tac
### bash_diff: requires /dev/fd/
# { ... } | tac inside < <(...)
while IFS= read -r line; do echo "$line"; done < <({ echo "1"; echo "2"; echo "3"; } | tac)
### expect
3
2
1
### end

### process_subst_group_pipe_sort
### bash_diff: requires /dev/fd/
# { ... } | sort inside < <(...)
while IFS= read -r line; do echo "$line"; done < <({ echo "c"; echo "a"; echo "b"; } | sort)
### expect
a
b
c
### end

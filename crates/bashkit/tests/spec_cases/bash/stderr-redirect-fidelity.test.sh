### command_not_found_honors_2_dev_null
# The command's redirects are set up before the name is looked up
nocmdq 2>/dev/null
echo "rc=$?"
### expect
rc=127
### end

### command_not_found_report_follows_2_to_1
out=$(nocmdq 2>&1)
echo "[$out]"
### expect
[bash: line 1: nocmdq: command not found]
### end

### missing_path_command_honors_2_dev_null
./missingq 2>/dev/null
echo "rc=$?"
### expect
rc=127
### end

### toplevel_return_report_follows_2_to_1
return 2>&1; echo "rc=$?"
### expect
bash: line 1: return: can only `return' from a function or sourced script
rc=2
### end

### source_missing_file_report_follows_2_to_1
out=$(source /nofileq 2>&1)
echo "[$out]"
### expect
[bash: line 1: /nofileq: No such file or directory]
### end

### builtin_not_a_shell_builtin_follows_2_to_1
out=$(builtin nocmdq 2>&1)
echo "[$out]"
### expect
[bash: line 1: builtin: nocmdq: not a shell builtin]
### end

### exit_non_numeric_argument
(exit abc) 2>/dev/null
echo "rc=$?"
### expect
rc=2
### end

### exit_without_argument_uses_last_status
(false; exit)
echo "rc=$?"
### expect
rc=1
### end

### exit_too_many_arguments_abandons_the_line
# Status 1 and the rest of the line is skipped (a subshell ends)
(exit 1 2; echo "same line") 2>/dev/null
echo "rc=$?"
### expect
rc=1
### end

### cmdsub_stderr_reaches_outer_stderr
out=$( { x=$(nocmdq); } 2>&1 )
echo "[$out]"
x=$(nocmdq 2>/dev/null; echo kept)
echo "x=[$x]"
### expect
[bash: line 1: nocmdq: command not found]
x=[kept]
### end

### cmdsub_stderr_not_hidden_by_command_redirect
# Words are expanded before the command's own redirects apply
out=$( { echo "a$(nocmdq)b" 2>/dev/null; } 2>&1 >/dev/null )
echo "[$out]"
### expect
[bash: line 1: nocmdq: command not found]
### end

### cmdsub_stderr_not_hidden_by_function_call_redirect
f() { :; }
out=$( { f "$(nocmdq)" 2>/dev/null; } 2>&1 )
echo "[$out]"
### expect
[bash: line 2: nocmdq: command not found]
### end

### cmdsub_own_redirect_captures_stderr
y=$(nocmdq 2>&1)
echo "[$y]"
### expect
[bash: line 1: nocmdq: command not found]
### end

### cmdsub_stderr_hidden_by_group_redirect
{ w=$(nocmdq); } 2>/dev/null
echo done
### expect
done
### end

### cmdsub_file_read_reports_missing_file
out=$( { q=$(< /nofileq); } 2>&1 )
echo "[$out]"
### expect
[bash: line 1: /nofileq: No such file or directory]
### end

### eval_keeps_caller_lineno
echo one
eval 'echo $LINENO'
eval 'eval "echo \$LINENO"'
### expect
one
2
3
### end

### eval_diagnostic_names_caller_line
echo one
out=$(eval 'nocmdq' 2>&1)
echo "[$out]"
### expect
one
[bash: line 2: nocmdq: command not found]
### end

### proc_refuses_new_files
{ echo y > /proc/naopode 2>/dev/null; } 2>/dev/null
echo "rc=$?"
[ -e /proc/naopode ] || echo absent
mkdir /proc/newdir 2>/dev/null || echo "mkdir failed"
touch /proc/newfile 2>/dev/null || echo "touch failed"
### expect
rc=1
absent
mkdir failed
touch failed
### end

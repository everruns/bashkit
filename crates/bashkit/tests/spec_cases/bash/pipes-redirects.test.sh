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

### redirect_stderr_suppress
# Suppress stderr with 2>/dev/null
sleep abc 2>/dev/null
echo exit: $?
### expect
exit: 1
### end

### redirect_stderr_suppress_ls
# Suppress stderr from ls with 2>/dev/null (issue #1116)
ls /nonexistent 2>/dev/null
echo exit: $?
### expect
exit: 2
### end

### redirect_stderr_suppress_compound
# Suppress stderr from compound command with 2>/dev/null (issue #1116)
{ ls /nonexistent; } 2>/dev/null
echo exit: $?
### expect
exit: 2
### end

### redirect_combined_suppress_ls
# Suppress both stdout and stderr with &>/dev/null (issue #1116)
ls /nonexistent &>/dev/null
echo exit: $?
### expect
exit: 2
### end

### redirect_stderr_to_file_content
# Redirect stderr content to file and verify it
sleep abc 2>/tmp/sleep_err.txt
cat /tmp/sleep_err.txt
### bash_diff: bashkit sleep error lacks --help hint from coreutils
### expect
sleep: invalid time interval 'abc'
### end

### redirect_stderr_dup_to_stdout
# Redirect stderr to stdout with 2>&1
sleep abc 2>&1 | cat
### bash_diff: pipe stderr propagation differs
### expect
sleep: invalid time interval 'abc'
### end

### redirect_both_to_file_content
# Redirect both stdout and stderr to file with &> and verify file content
{ echo hello; sleep abc; } &>/tmp/both_content.txt
cat /tmp/both_content.txt
### bash_diff: bashkit sleep error lacks --help hint from coreutils
### expect
hello
sleep: invalid time interval 'abc'
### end

### redirect_both_devnull
# Suppress both stdout and stderr with &>/dev/null
echo output &>/dev/null
echo done
### expect
done
### end

### redirect_stderr_append_content
# Append stderr from multiple commands
sleep abc 2>/tmp/err_multi.txt; sleep xyz 2>>/tmp/err_multi.txt; cat /tmp/err_multi.txt
### bash_diff: bashkit sleep error lacks --help hint from coreutils
### expect
sleep: invalid time interval 'abc'
sleep: invalid time interval 'xyz'
### end

### redirect_stdout_to_stderr
# Redirect stdout to stderr (>&2) then suppress stderr
echo hello >&2 2>/dev/null
echo done
### bash_diff: redirect ordering model differs
### expect
done
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

### readwrite_redirect_opens_file
### skip: `<>` (read-write fd) is not implemented — parser rejects the operator
# Bash opens the file read-write on fd 0 without truncating it.
echo start > /tmp/rw.txt
exec 3<> /tmp/rw.txt
cat <&3
### expect
start
### end

### null_command_output_redirect_truncates
# A redirect with no command still truncates/creates the file
echo data > /tmp/nullcmd_out
> /tmp/nullcmd_out
wc -c < /tmp/nullcmd_out
echo "rc=$?"
### expect
0
rc=0
### end

### null_command_input_redirect_missing
# A lone input redirect to a missing file fails with status 1
{ < /tmp/nullcmd_missing; } 2>/dev/null
echo "rc=$?"
### expect
rc=1
### end

### null_command_redirect_resets_status
# A successful redirect-only command exits 0
false
> /tmp/nullcmd_status
echo "rc=$?"
### expect
rc=0
### end

### assignment_with_redirect_creates_file
# x=1 > file assigns and still creates the file
x=1 > /tmp/nullcmd_assign
echo "$x"
test -f /tmp/nullcmd_assign && echo exists
### expect
1
exists
### end

### pipeline_last_stage_is_subshell
# Without lastpipe every pipeline stage runs in a subshell
echo x | read v
echo "${v-unset}"
n=0
printf 'a\nb\n' | while read -r l; do n=$((n+1)); done
echo "n=$n"
### expect
unset
n=0
### end

### pipeline_lastpipe_runs_last_stage_in_shell
# shopt -s lastpipe keeps the last stage in the current shell
shopt -s lastpipe
echo x | read v
echo "v=$v"
n=0
printf 'a\nb\n' | while read -r l; do n=$((n+1)); done
echo "n=$n"
### expect
v=x
n=2
### end

### pipeline_first_stage_state_does_not_leak
# Assignments and cd in a non-last stage stay in that stage
x=1
x=2 | cat
cd /
cd /tmp | true
echo "x=$x pwd=$PWD"
### expect
x=1 pwd=/
### end

### pipeline_stage_exit_only_ends_the_stage
# exit inside a pipeline stage ends that stage, not the shell
echo hi | exit 3
echo "after $?"
exit 4 | cat
echo "still here"
### expect
after 3
still here
### end

### pipeline_endless_producer_into_head
# head exits after two lines; the endless loop gets SIGPIPE (141)
while :; do echo x; done | head -n 2
echo "${PIPESTATUS[*]}"
### expect
x
x
141 0
### end

### pipeline_endless_producer_into_reads
# A group that reads two lines and leaves ends the producer too
i=0
while :; do echo $((i++)); done | { read -r a; read -r b; echo "$a $b"; }
echo "${PIPESTATUS[*]}"
### expect
0 1
141 0
### end

### pipeline_endless_through_middle_stage
# Every stage upstream of head is ended by SIGPIPE
while :; do echo a; done | while read -r l; do echo "<$l>"; done | head -n 2
echo "${PIPESTATUS[*]}"
### expect
<a>
<a>
141 141 0
### end

### pipeline_reader_breaks_out_early
# A while-read loop that breaks ends an endless producer
while :; do echo y; done | while read -r l; do echo "got $l"; break; done
echo "${PIPESTATUS[*]}"
### expect
got y
141 0
### end

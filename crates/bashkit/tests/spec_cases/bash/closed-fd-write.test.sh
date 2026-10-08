### write_to_closed_stdout_drops_the_output_and_fails
### exit_code: 1
exec 1>&- 2>&-
echo x
### end

### closing_stderr_alone_keeps_stdout_and_fails_the_stderr_write
exec 2>&-
echo x
echo y >&2
echo "st=$?"
### expect
x
st=1
### end

### exec_inside_a_subshell_redirects_the_subshell
( exec > log.txt; echo inside )
echo sep
cat log.txt
### expect
sep
inside
### end

### output_before_the_subshell_exec_reaches_the_caller
( echo pre; exec > log.txt; echo post )
echo sep
cat log.txt
### expect
pre
sep
post
### end

### a_subshell_can_redirect_its_stderr_with_exec
( exec 2>err.txt; ls /nope )
cat err.txt
### expect
ls: cannot access '/nope': No such file or directory
### end

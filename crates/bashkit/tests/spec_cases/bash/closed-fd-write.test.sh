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

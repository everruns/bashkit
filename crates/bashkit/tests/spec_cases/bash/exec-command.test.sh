### exec_replaces_execution
# exec stops subsequent statements
cat > /tmp/greeter.sh <<'SCRIPT'
#!/usr/bin/env bash
echo "hello from greeter"
SCRIPT
chmod +x /tmp/greeter.sh

cat > /tmp/dispatcher.sh <<'SCRIPT'
#!/usr/bin/env bash
echo "before exec"
exec /tmp/greeter.sh
echo "SHOULD NOT APPEAR"
SCRIPT
chmod +x /tmp/dispatcher.sh

/tmp/dispatcher.sh
### expect
before exec
hello from greeter
### end

### exec_propagates_exit_code
# exec propagates exit code from executed command
cat > /tmp/exit-42.sh <<'SCRIPT'
#!/usr/bin/env bash
exit 42
SCRIPT
chmod +x /tmp/exit-42.sh

cat > /tmp/exec-it.sh <<'SCRIPT'
#!/usr/bin/env bash
exec /tmp/exit-42.sh
SCRIPT
chmod +x /tmp/exec-it.sh

/tmp/exec-it.sh
echo $?
### expect
42
### end

### exec_with_builtin
# exec with builtin command
cat > /tmp/exec-echo.sh <<'SCRIPT'
#!/usr/bin/env bash
exec echo "via exec"
echo "SHOULD NOT APPEAR"
SCRIPT
chmod +x /tmp/exec-echo.sh

/tmp/exec-echo.sh
### expect
via exec
### end

### exec_passes_arguments
# exec passes arguments to command
cat > /tmp/echo-args.sh <<'SCRIPT'
#!/usr/bin/env bash
echo "args: $*"
SCRIPT
chmod +x /tmp/echo-args.sh

cat > /tmp/exec-args.sh <<'SCRIPT'
#!/usr/bin/env bash
exec /tmp/echo-args.sh one two three
SCRIPT
chmod +x /tmp/exec-args.sh

/tmp/exec-args.sh
### expect
args: one two three
### end

### exec_fd_redirections_still_work
# exec without command still does FD redirections
echo "file content" > /tmp/exec-test-file.txt
exec 3< /tmp/exec-test-file.txt
echo "after exec redirect"
### expect
after exec redirect
### end

### exec_stdout_to_file_then_restore
exec 3>&1 > /tmp/ex.log 2>&1
echo one
ls /nonexistent_zz
echo two >&3
exec 1>&3 3>&-
echo back
cat /tmp/ex.log
### expect
two
back
one
ls: cannot access '/nonexistent_zz': No such file or directory
### end

### exec_inside_function_persists
exec 4>&1
main() { exec > /tmp/m.log; echo inside; }
main
echo after
exec >&4 4>&-
echo restored
cat /tmp/m.log
### expect
restored
inside
after
### end

### exec_redirect_spares_command_substitution
exec 3>&1 > /tmp/c.log
x=$(echo captured; ls /nonexistent_zz 2>&1 | wc -l)
echo "x=$x" | tr '\n' ' ' >&3
echo
exec >&3
echo "log=[$(cat /tmp/c.log)]"
### expect
x=captured 1 log=[]
### end

### exec_stderr_to_dev_null
exec 2>/dev/null
ls /nonexistent_zz
echo "rc=$?"
### expect
rc=2
### end

### exec_append_and_pipeline
echo first > /tmp/a.log
exec 3>&1 >> /tmp/a.log
printf 'b\na\n' | sort
for i in 1 2; do echo "loop $i"; done
exec >&3
cat /tmp/a.log
### expect
first
a
b
loop 1
loop 2
### end

# env runs a command in a modified environment, like a child process.

### env_assignment_scoped_to_command
export FOO=outer
env FOO=1 bash -c 'echo $FOO'
echo after=$FOO
### expect
1
after=outer
### end

### env_unset_and_ignore
export FOO=outer
env -u FOO bash -c 'echo "[$FOO]"'
env -i A=1 env
env -i bash -c 'echo "[$HOME]"'
### expect
[]
A=1
[]
### end

### env_overrides_without_duplicates
export FOO=outer
env FOO=x env | grep -c '^FOO='
env A=1 B=2 env | grep -E '^(A|B)='
### expect
1
A=1
B=2
### end

### env_chdir_and_stdin
d=$(mktemp -d)
env -C "$d" pwd | grep -c "$d"
echo piped | env cat
env -- echo hi
### expect
1
piped
hi
### end

### env_errors
env nosuch 2>&1; echo rc=$?
env exit 3 2>&1; echo still=$?
env -C /nonexistent pwd 2>&1; echo rc=$?
### expect
env: 'nosuch': No such file or directory
rc=127
env: 'exit': No such file or directory
still=127
env: cannot change directory to '/nonexistent': No such file or directory
rc=125
### end

### type_builtin
# type reports builtins
type echo
### expect
echo is a shell builtin
### end

### type_keyword
# type reports keywords
type if
### expect
if is a shell keyword
### end

### type_function
# type reports functions with their body
myfunc() { echo hi; }
type myfunc
### expect
myfunc is a function
myfunc () 
{ 
    echo hi
}
### end

### type_not_found
### exit_code:1
# type exits 1 for unknown command; the error goes to stderr
type nonexistent_cmd_xyz
### expect
### end

### type_t_builtin
# type -t prints just the type word
type -t echo
### expect
builtin
### end

### type_t_keyword
# type -t for keyword
type -t for
### expect
keyword
### end

### type_t_function
# type -t for function
myfunc() { echo hi; }
type -t myfunc
### expect
function
### end

### type_t_not_found
### exit_code:1
# type -t prints nothing for unknown
type -t nonexistent_cmd_xyz
### expect
### end

### type_multiple
# type handles multiple names
type echo true
### expect
echo is a shell builtin
true is a shell builtin
### end

### type_a_builtin
# type -a shows the builtin, then every PATH match
type -a echo
### expect
echo is a shell builtin
echo is /usr/bin/echo
echo is /bin/echo
### end

### which_builtin
# which searches PATH; the root filesystem provides /usr/bin/echo
which echo
### expect
/usr/bin/echo
### end

### which_not_found
### exit_code:1
# which exits 1 for unknown command
which nonexistent_cmd_xyz
### expect
### end

### which_multiple
# which handles multiple names
which echo cat
### expect
/usr/bin/echo
/usr/bin/cat
### end

### which_function
### exit_code:1
# which only searches PATH, not functions
myfunc() { echo hi; }
which myfunc
### expect
### end

### type_external_command
# Commands that are programs on a real system resolve to /usr/bin
type ls
type -t ls
command -v ls
command -V cat
type -P echo
### expect
ls is /usr/bin/ls
file
/usr/bin/ls
cat is /usr/bin/cat
/usr/bin/echo
### end

### hash_noop
### bash_diff: real bash prints hash table contents
# hash is a no-op in sandboxed env
hash
echo "ok"
### expect
ok
### end

### rootfs_layout
# Default root filesystem: /etc, /proc and command stubs
grep -c '^processor' /proc/cpuinfo
grep '^ID=' /etc/os-release
cut -d: -f1 /etc/passwd
cat /etc/hostname
/usr/bin/env A=1 printenv A
[ -x /usr/bin/env ] && echo env-exec
head -c 4 /dev/zero | od -An -c
echo hi > /etc/passwd 2>/dev/null || echo denied
mkdir -p /etc/myapp && echo conf > /etc/myapp/x && cat /etc/myapp/x
### bash_diff: synthetic identity (sandbox user, bashkit-sandbox host, 4 CPUs)
### expect
4
ID=bashkit
sandbox
nobody
bashkit-sandbox
1
env-exec
  \0  \0  \0  \0
denied
conf
### end

### rootfs_proc_random_uuid
# /proc/sys/kernel/random/uuid yields a fresh random UUID per read
a=$(cat /proc/sys/kernel/random/uuid)
b=$(cat /proc/sys/kernel/random/uuid)
[[ $a =~ ^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$ ]] && echo shape
[ "$a" != "$b" ] && echo fresh
### expect
shape
fresh
### end

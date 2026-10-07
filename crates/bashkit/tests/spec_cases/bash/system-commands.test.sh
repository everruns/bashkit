# Commonly scripted coreutils/util-linux commands: arch, sum, shasum,
# egrep/fgrep, link/unlink, chgrp, nohup/nice/flock, getconf, tty, sync,
# plus virtual-identity commands (groups, logname, users, who, uptime, free,
# hostid).

### arch_machine
arch
### expect
x86_64
### end

### sum_bsd_and_sysv
d=$(mktemp -d); cd "$d"
printf 'hello\n' > h
sum h; sum -r h; sum -s h; printf 'hello\n' | sum
head -c 3000 /dev/zero | tr '\0' x > big
sum big; sum -s big
### expect
36979     1 h
36979     1 h
542 1 h
36979     1
05357     3 big
32325 6 big
### end

### shasum_algorithms
printf 'hello\n' | shasum
printf 'hello\n' | shasum -a 256
printf 'hello\n' | shasum -a 1 | cut -c1-8
### expect
f572d396fae9206628714fb2ce00f72e94f2258f  -
5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03  -
f572d396
### end

### egrep_fgrep
printf 'a\nb\nfoo\n' | egrep 'a|foo' 2>/dev/null
printf 'a.b\naxb\n' | fgrep 'a.b' 2>/dev/null
printf 'x\n' | fgrep -c y 2>/dev/null; echo rc=$?
### expect
a
foo
a.b
0
rc=1
### end

### link_unlink
d=$(mktemp -d); cd "$d"
printf 'hello\n' > h
link h h2; cat h2
unlink h2; [ -e h2 ] || echo gone
unlink missing 2>/dev/null; echo rc=$?
### expect
hello
gone
rc=1
### end

### chgrp_same_group
d=$(mktemp -d); cd "$d"
touch f
chgrp "$(id -gn)" f; echo rc=$?
### expect
rc=0
### end

### nohup_nice_run_command
nohup echo hi 2>/dev/null
nice -n 5 echo nice
nice -n 5 sh -c 'exit 3'; echo rc=$?
nohup sh -c 'exit 4' 2>/dev/null; echo rc=$?
### expect
hi
nice
rc=3
rc=4
### end

### nice_without_command
nice
### expect
0
### end

### nohup_missing_command
nohup 2>/dev/null; echo rc=$?
nice -n 2>/dev/null; echo rc=$?
### expect
rc=125
rc=125
### end

### flock_runs_command
d=$(mktemp -d); cd "$d"
flock lk echo locked
flock lk -c 'echo inner; exit 2'; echo rc=$?
[ -f lk ] && echo lockfile
### expect
locked
inner
rc=2
lockfile
### end

### flock_fd_form
d=$(mktemp -d); cd "$d"
( flock -n 9 && echo got ) 9>lk
### expect
got
### end

### getconf_common
getconf PAGE_SIZE
getconf PAGESIZE
getconf LONG_BIT
getconf CHAR_BIT
getconf NOT_A_VAR 2>/dev/null; echo rc=$?
### expect
4096
4096
64
8
rc=2
### end

### tty_not_a_terminal
tty < /dev/null; echo rc=$?
tty -s < /dev/null; echo rc=$?
### expect
not a tty
rc=1
rc=1
### end

### sync_succeeds
sync; echo rc=$?
### expect
rc=0
### end

### type_dispatch_builtins
type builtin
type -t let
command -V typeset
### expect
builtin is a shell builtin
builtin
typeset is a shell builtin
### end

### shells_have_bin_paths
[ -x /bin/sh ] && echo sh
[ -x /bin/bash ] && echo bash
/bin/sh -c 'echo via-sh'
### expect
sh
bash
via-sh
### end

### groups_logname_virtual_user
### bash_diff: bashkit reports its configured virtual user, never the host's
groups
logname
groups "$(whoami)"
### expect
sandbox
sandbox
sandbox : sandbox
### end

### users_who_empty
### bash_diff: no login sessions exist in the sandbox
users; who; echo end
### expect
end
### end

### free_virtual_memory
### bash_diff: bashkit reports fixed virtual memory, never the host's
free | head -1 | tr -s ' '
free -m | awk 'NR==2 {print $1, $2}'
### expect
 total used free shared buff/cache available
Mem: 4096
### end

### uptime_shape
### bash_diff: uptime is derived from the virtual clock
uptime -p | grep -q '^up ' && echo ok
### expect
ok
### end

### hostid_fixed
### bash_diff: bashkit reports a fixed virtual host id
hostid
### expect
007f0101
### end

# dd, install, umask, ulimit, builtin, enable, locale.

### dd_skip_count_and_conv
printf 'hello world\n' | dd bs=1 skip=6 count=5 status=none; echo
printf 'abcdef' | dd bs=2 count=2 2>/dev/null; echo
printf 'abc' | dd conv=ucase status=none; echo
### expect
world
abcd
ABC
### end

### dd_records_report
printf 'abcdef' | dd bs=4 2>&1 >/dev/null | head -2
### expect
1+1 records in
1+1 records out
### end

### dd_zero_device_and_seek
d=$(mktemp -d); cd "$d"
dd if=/dev/zero bs=1K count=3 of=z status=none; wc -c < z
printf 'XY' | dd of=z bs=1 seek=1 conv=notrunc status=none
od -An -c z | head -1
dd if=/dev/urandom bs=16 count=2 status=none | wc -c
### expect
3072
  \0   X   Y  \0  \0  \0  \0  \0  \0  \0  \0  \0  \0  \0  \0  \0
32
### end

### install_modes_and_dirs
d=$(mktemp -d); cd "$d"
printf 'x' > src
install -m 640 src dst; stat -c %a dst
install -d -m 700 d1/d2; stat -c %a d1/d2
install -D src deep/a/b; cat deep/a/b; echo
install -v src d1
install nope dst 2>&1; echo rc=$?
### expect
640
700
x
'src' -> 'd1/src'
install: cannot stat 'nope': No such file or directory
rc=1
### end

### umask_report_and_set
umask; umask -S
umask 077; umask; umask -p
umask u=rwx,g=rx,o=; umask
(umask 000); umask
### expect
0022
u=rwx,g=rx,o=rx
0077
umask 0077
0027
0027
### end

### ulimit_set_and_lower
ulimit -n 100; ulimit -n; ulimit -Hn
ulimit -n 200 2>/dev/null; echo rc=$?
ulimit
### expect
100
100
rc=1
unlimited
### end

### builtin_bypasses_functions
echo() { printf 'func\n'; }
echo x
builtin echo y
unset -f echo
builtin nosuch 2>/dev/null; echo rc=$?
### expect
func
y
rc=1
### end

### enable_checks_names
enable echo; echo rc=$?
enable nosuch 2>/dev/null; echo rc=$?
### expect
rc=0
rc=1
### end

### locale_default_posix
unset LANG LC_ALL LC_CTYPE
locale | sed -n '1p;3p;$p'
locale -a | grep -c POSIX
### expect
LANG=
LC_CTYPE="POSIX"
LC_ALL=
1
### end

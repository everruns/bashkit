# sudo runs its command (one sandbox user, no privilege boundary) and
# busybox runs the builtin named by its first argument. Agent scripts use
# both; without them the whole command failed with 127.

### sudo_runs_command
### bash_diff: sudo on the reference host may prompt or change identity
sudo tee /tmp/sudo.conf > /dev/null <<EOF
key=1
EOF
cat /tmp/sudo.conf
sudo -E sh -c 'echo from-shell'
sudo -n mkdir -p /tmp/sudo-dir && echo made
### expect
key=1
from-shell
made
### end

### sudo_env_assignment_and_shell
### bash_diff: sudo on the reference host may prompt or change identity
sudo FOO=bar printenv FOO
sudo -s echo via -s
sudo -v; echo "v=$?"
### expect
bar
via -s
v=0
### end

### sudo_missing_command
### bash_diff: sudo on the reference host may prompt or change identity
sudo 2>/dev/null; echo "rc=$?"
### expect
rc=1
### end

### busybox_runs_applet
### bash_diff: busybox is not installed on the reference host
busybox echo hi
busybox /bin/echo path
printf 'b\na\n' | busybox sort
busybox --list | grep -qx tar && echo listed
### expect
hi
path
a
b
listed
### end

### runner_help_belongs_to_command
### bash_diff: help text differs from GNU coreutils
nohup tar --help | head -1
### expect
Usage: tar [OPTION]... [FILE]...
### end

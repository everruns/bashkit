# /proc/self and friends: synthetic process view of the sandbox shell.

### proc_self_status_identity
### bash_diff: synthetic /proc (pid 1, uid 1000)
grep -E '^(Name|Pid|PPid|Uid):' /proc/self/status
### expect
Name:	bash
Pid:	1
PPid:	0
Uid:	1000	1000	1000	1000
### end

### proc_self_comm_and_cmdline
### bash_diff: synthetic /proc
cat /proc/self/comm
tr '\0' ' ' < /proc/self/cmdline; echo
### expect
bash
bash 
### end

### proc_pid_matches_self
### bash_diff: synthetic /proc (pid 1)
cmp -s /proc/self/status /proc/$$/status && echo same
### expect
same
### end

### proc_self_exe_and_cgroup
### bash_diff: synthetic /proc
readlink /proc/self/exe
cat /proc/self/cgroup
### expect
/bin/bash
0::/
### end

### proc_mounts_and_uptime
### bash_diff: synthetic /proc
cat /proc/mounts
read up idle < /proc/uptime && [[ $up =~ ^[0-9]+\.[0-9]{2}$ ]] && echo uptime-ok
### expect
bashkit-vfs / bashkit-vfs rw 0 0
uptime-ok
### end

### proc_ls_lists_self
### bash_diff: synthetic /proc
ls /proc | grep -xE 'self|1|uptime|mounts' | sort
### expect
1
mounts
self
uptime
### end

### proc_files_read_only
### bash_diff: synthetic /proc
echo x > /proc/self/comm 2>/dev/null || echo denied
### expect
denied
### end

### bashpid_is_shell_pid
[ "$BASHPID" = "$$" ] && echo same
### expect
same
### end

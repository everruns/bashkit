### kill_self_status_per_signal
bash -c 'kill -9 $$' 2>/dev/null; echo "kill9=$?"
bash -c 'kill -TERM $$' 2>/dev/null; echo "term=$?"
bash -c 'kill -INT $$' 2>/dev/null; echo "int=$?"
bash -c 'kill -0 $$; echo zero=$?' 2>/dev/null
### expect
kill9=137
term=143
int=130
zero=0
### end

### kill_self_runs_the_trap
trap 'echo caught-USR1' USR1
kill -USR1 $$
echo still here
trap 'echo caught-TERM' TERM
kill -TERM $$
echo "after=$?"
### expect
caught-USR1
still here
caught-TERM
after=0
### end

### kill_self_trap_can_exit
bash -c 'trap "echo bye; exit 143" TERM; kill -TERM $$; echo never'
echo "rc=$?"
### expect
bye
rc=143
### end

### kill_self_ignored_signal
kill -CHLD $$
echo alive
### expect
alive
### end

### nested_self_signals_return_to_each_handler
trap 'echo first; kill -USR2 $$; echo last' USR1
trap 'echo second' USR2
kill -USR1 $$
kill -USR1 $$
echo done
### expect
first
second
last
first
second
last
done
### end

### ignored_self_signal_and_handler_exit
trap 'echo ignored' USR1
trap '' USR1
kill -USR1 $$
trap 'echo caught; exit 7' USR2
kill -USR2 $$
echo unreachable
### expect
caught
### exit_code: 7
### end

### jobs_run_concurrently
# two one-second sleeps in the background finish in about one second
start=$(date +%s%N)
sleep 1 & sleep 1 & wait
end=$(date +%s%N)
(( (end - start) / 1000000 < 1900 )) && echo concurrent
### expect
concurrent
### end

### jobs_bg_output_after_foreground
# a blocking job reports after the foreground output that ran first
(sleep 0.2; echo bg) &
echo fg
wait
### expect
fg
bg
### end

### jobs_wait_returns_job_status
(sleep 0.1; exit 3) &
pid=$!
wait $pid
echo "code=$?"
### expect
code=3
### end

### jobs_wait_unknown_pid
wait 99999
echo "code=$?"
### expect
code=127
### end

### jobs_wait_n_picks_first_finished
sleep 0.4 &
(sleep 0.1; exit 7) &
wait -n
echo "first=$?"
wait -n
echo "second=$?"
wait -n
echo "none=$?"
### expect
first=7
second=0
none=127
### end

### jobs_kill_running_job
sleep 10 &
kill $!
wait $!
echo "code=$?"
### expect
code=143
### end

### jobs_kill_signal_forms
sleep 10 &
kill -9 %1
wait %1 2>/dev/null
echo "k9=$?"
sleep 10 &
kill -s INT $!
wait $!
echo "int=$?"
kill -l 143
### expect
k9=137
int=130
TERM
### end

### jobs_kill_unknown_pid
kill 99999
echo "code=$?"
### expect
code=1
### end

### jobs_list_running
sleep 2 & sleep 3 &
true &
sleep 0.1
jobs
jobs -p | wc -l
kill %1 %2
wait
### expect
[1]-  Running                 sleep 2 &
[2]+  Running                 sleep 3 &
2
### end

### jobs_numbers_reused_after_finish
sleep 0.1 &
wait
sleep 2 &
jobs
kill %1
### expect
[1]+  Running                 sleep 2 &
### end

### jobs_variables_are_forked
x=1
(x=2; echo "in=$x") &
wait
echo "out=$x"
### expect
in=2
out=1
### end

### jobs_pgrep_pkill
### bash_diff: real pgrep also sees host processes
sleep 10 &
sleep 10 &
pgrep -c sleep
pkill sleep
wait
pgrep sleep || echo none
### expect
2
none
### end

### trailing_ampersand_in_compound
# The last `cmd &` before `}`, `done`, `fi` or `)` still runs in background.
f() { sleep 0.3 & }
start=$(date +%s%N)
f
{ sleep 0.3 & }
for i in 1; do sleep 0.3 & done
if true; then sleep 0.3 & fi
end=$(date +%s%N)
(( (end - start) / 1000000 < 250 )) && echo async
jobs | wc -l
wait
### expect
async
4
### end

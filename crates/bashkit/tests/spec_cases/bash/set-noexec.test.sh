### set_n_stops_execution
echo a
set -n
echo not-run
set +n
echo still-not-run
### expect
a
### end

### set_o_noexec_stops_execution
echo a; set -o noexec; echo b
### expect
a
### end

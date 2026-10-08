### return_outside_a_function_reports_and_keeps_going
return 2>/dev/null
echo "rc=$?"
echo after
### expect
rc=2
after
### end

### return_inside_a_function_still_works
f() { return 3; }
f
echo "rc=$?"
### expect
rc=3
### end

### return_in_a_sourced_script_is_its_status
printf 'echo in\nreturn 4\necho no\n' > s.sh
. ./s.sh
echo "rc=$?"
### expect
in
rc=4
### end

### unset_v_insists_on_an_identifier
unset -v 'arr[' 2>/dev/null
echo "rc=$?"
### expect
rc=1
### end

### unset_without_v_accepts_any_word
unset 'arr['
unset -f 'f['
echo "rc=$?"
### expect
rc=0
### end

### unset_v_still_takes_a_subscript
a=(x y)
unset -v 'a[0]'
echo "${a[@]}"
### expect
y
### end

### read_rejects_a_non_numeric_timeout
read -t abc x <<< '' 2>/dev/null
echo "rc=$?"
### expect
rc=1
### end

### read_rejects_a_non_numeric_count
read -n abc x <<< '' 2>/dev/null
echo "rc=$?"
### expect
rc=1
### end

### read_still_takes_a_numeric_count
read -n 3 x <<< 'hello'
echo "$x"
### expect
hel
### end

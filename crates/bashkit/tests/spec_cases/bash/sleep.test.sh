### sleep_zero
# Sleep for 0 seconds should return immediately
sleep 0
echo $?
### expect
0
### end

### sleep_fractional
# Sleep for fractional seconds
sleep 0.01
echo done
### expect
done
### end

### sleep_integer
# Sleep with integer argument
sleep 0
echo $?
### expect
0
### end

### sleep_missing_operand
# Sleep without argument should error
sleep
echo exit: $?
### expect
exit: 1
### end

### sleep_invalid_argument
# Sleep with invalid argument should error
sleep abc
echo exit: $?
### expect
exit: 1
### end

### sleep_negative
# Sleep with negative value should error
sleep -1
echo exit: $?
### expect
exit: 1
### end

### sleep_stderr_suppress
# Suppress sleep error message with 2>/dev/null
sleep abc 2>/dev/null
echo exit: $?
### expect
exit: 1
### end

### sleep_stderr_to_file
# Redirect sleep error message to file
sleep abc 2>/tmp/sleep_err.txt; cat /tmp/sleep_err.txt
### expect
sleep: invalid time interval 'abc'
### end

### sleep_stderr_append
# Append stderr from multiple sleep errors
sleep abc 2>/tmp/sleep_errs.txt; sleep xyz 2>>/tmp/sleep_errs.txt
cat /tmp/sleep_errs.txt
### expect
sleep: invalid time interval 'abc'
sleep: invalid time interval 'xyz'
### end

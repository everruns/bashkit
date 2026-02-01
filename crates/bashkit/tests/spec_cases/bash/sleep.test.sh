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
### skip: stderr redirect not implemented
# Sleep without argument should error
sleep
echo exit: $?
### expect
exit: 1
### end

### sleep_invalid_argument
### skip: stderr redirect not implemented
# Sleep with invalid argument should error
sleep abc
echo exit: $?
### expect
exit: 1
### end

### sleep_negative
### skip: stderr redirect not implemented
# Sleep with negative value should error
sleep -1
echo exit: $?
### expect
exit: 1
### end

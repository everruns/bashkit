### seq_single_arg
# seq LAST - count from 1 to LAST
seq 5
### expect
1
2
3
4
5
### end

### seq_two_args
# seq FIRST LAST
seq 3 6
### expect
3
4
5
6
### end

### seq_three_args
# seq FIRST INCREMENT LAST
seq 1 2 9
### expect
1
3
5
7
9
### end

### seq_decrement
# seq counting down
seq 5 -1 1
### expect
5
4
3
2
1
### end

### seq_negative
# seq with negative numbers
seq -2 2
### expect
-2
-1
0
1
2
### end

### seq_equal_width
# seq -w pads with leading zeros
seq -w 1 10
### expect
01
02
03
04
05
06
07
08
09
10
### end

### seq_separator
# seq -s uses custom separator
seq -s ", " 1 5
### expect
1, 2, 3, 4, 5
### end

### seq_single_value
# seq 1 produces just 1
seq 1
### expect
1
### end

### seq_no_output
# seq where FIRST > LAST produces no output
seq 5 1
echo "done"
### expect
done
### end

### seq_in_subst
# seq output captured in command substitution
result=$(seq 3)
echo "$result"
### expect
1
2
3
### end

### seq_step_two
# seq with step of 2
seq 0 2 8
### expect
0
2
4
6
8
### end

### seq_missing_operand
# seq with no args should error
seq 2>/dev/null
echo $?
### expect
1
### end

### arith_division_by_zero_aborts_and_fails
( echo before; echo $((1/0)); echo never ) 2>/dev/null
echo "rc=$?"
### expect
before
rc=1
### end

### arith_error_messages
x=$( { echo $((1 % 0)); } 2>&1 ); echo "${x#bash: line *: }"
y=$( { echo $((08)); } 2>&1 ); echo "${y#bash: line *: }"
z=$( { echo $((2#12)); } 2>&1 ); echo "${z#bash: line *: }"
### expect
1 % 0: division by 0 (error token is "0")
08: value too great for base (error token is "08")
2#12: value too great for base (error token is "2#12")
### end

### arith_command_and_let_report_and_continue
m=$( { ((1/0)); } 2>&1 ); echo "${m#bash: line *: }"
((1/0)); echo "cmd rc=$?"
n=$( { let "y=2#12"; } 2>&1 ); echo "${n#bash: line *: }"
let "y=1/0"; echo "let rc=$?"
echo done
### expect
((: 1/0: division by 0 (error token is "0")
cmd rc=1
let: y=2#12: value too great for base (error token is "2#12")
let rc=1
done
### end

### arith_valid_bases_still_work
echo $((0x1f)) $((010)) $((16#ff)) $((10#08)) $((2#101)) $((64#@)) $(( -1 / 1 ))
a=5; ((a+=2)); let b=a*2; echo $a $b
### expect
31 8 255 8 5 62 -1
7 14
### end

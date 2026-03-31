# Exit status tests
# Inspired by Oils spec/exit-status.test.sh
# https://github.com/oilshell/oil/blob/master/spec/exit-status.test.sh

### exit_truncation_255
# exit 255 is preserved
bash -c 'exit 255'
echo status=$?
### expect
status=255
### end

### exit_truncation_256
# exit 256 truncates to 0
bash -c 'exit 256'
echo status=$?
### expect
status=0
### end

### exit_truncation_257
# exit 257 truncates to 1
bash -c 'exit 257'
echo status=$?
### expect
status=1
### end

### exit_negative_minus1
# exit -1 wraps to 255
bash -c 'exit -1' 2>/dev/null
echo status=$?
### expect
status=255
### end

### exit_negative_minus2
# exit -2 wraps to 254
bash -c 'exit -2' 2>/dev/null
echo status=$?
### expect
status=254
### end

### return_truncation_255
# return 255 is preserved
f() { return 255; }; f
echo status=$?
### expect
status=255
### end

### return_truncation_256
# return 256 truncates to 0
f() { return 256; }; f
echo status=$?
### expect
status=0
### end

### return_truncation_257
# return 257 truncates to 1
f() { return 257; }; f
echo status=$?
### expect
status=1
### end

### return_negative_minus1
# return -1 wraps to 255
f() { return -1; }; f 2>/dev/null
echo status=$?
### expect
status=255
### end

### return_negative_minus2
# return -2 wraps to 254
f() { return -2; }; f 2>/dev/null
echo status=$?
### expect
status=254
### end

### if_empty_command
# If empty command string - '' as command should fail
if ''; then echo TRUE; else echo FALSE; fi
### exit_code: 0
### expect
FALSE
### end

### empty_command_sub_exit_code
# Exit code propagation through empty command sub
`true`; echo $?
`false`; echo $?
$(true); echo $?
$(false); echo $?
### expect
0
1
0
1
### end

### empty_argv_with_command_sub
# More test cases with empty argv from command sub
true $(false)
echo status=$?
$(exit 42)
echo status=$?
### expect
status=0
status=42
### end

### pipeline_exit_status
# Pipeline exit status is last command
true | false
echo $?
false | true
echo $?
### expect
1
0
### end

### and_list_exit_status
# AND list exit status
true && true; echo $?
true && false; echo $?
false && true; echo $?
### expect
0
1
1
### end

### or_list_exit_status
# OR list exit status
true || true; echo $?
true || false; echo $?
false || true; echo $?
false || false; echo $?
### expect
0
0
0
1
### end

### subshell_exit_code
# Subshell preserves exit code
(exit 0); echo $?
(exit 1); echo $?
(exit 42); echo $?
### expect
0
1
42
### end

### negation_exit_status
# ! negates exit status
! true; echo $?
! false; echo $?
### expect
1
0
### end

### exit_in_if_block
# exit inside if block terminates script
if true; then echo before; exit 0; fi
echo SHOULD_NOT_REACH
### expect
before
### end

### exit_in_if_block_with_code
# exit with non-zero code inside if block
if true; then exit 42; fi
echo SHOULD_NOT_REACH
### exit_code: 42
### expect
### end

### exit_in_while_loop
# exit inside while loop terminates script
i=0
while true; do
  echo "iter $i"
  if [ "$i" -eq 1 ]; then exit 0; fi
  i=$((i + 1))
done
echo SHOULD_NOT_REACH
### expect
iter 0
iter 1
### end

### exit_in_for_loop
# exit inside for loop terminates script
for x in a b c; do
  echo "$x"
  if [ "$x" = "b" ]; then exit 5; fi
done
echo SHOULD_NOT_REACH
### exit_code: 5
### expect
a
b
### end

### exit_in_case_block
# exit inside case block terminates script
case "yes" in
  yes) echo matched; exit 0 ;;
esac
echo SHOULD_NOT_REACH
### expect
matched
### end

### exit_in_subshell_does_not_propagate
# exit inside subshell only terminates the subshell
(exit 7)
echo "after subshell: $?"
### expect
after subshell: 7
### end

### exit_in_function
# exit inside function terminates the whole script
f() { echo in_func; exit 3; }
f
echo SHOULD_NOT_REACH
### exit_code: 3
### expect
in_func
### end

### if_condition_stdout
# stdout from if condition is preserved
if echo "from_condition"; then echo "from_body"; fi
### expect
from_condition
from_body
### end

### if_negated_condition_stdout
# stdout from negated if condition
if ! echo "negated"; then echo "no"; else echo "yes"; fi
### expect
negated
yes
### end

### if_condition_pipeline_stdout
# stdout from pipeline in if condition
if echo "hello" | cat; then echo "ok"; fi
### expect
hello
ok
### end

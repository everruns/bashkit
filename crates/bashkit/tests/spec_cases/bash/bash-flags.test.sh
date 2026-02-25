### bash_e_errexit
# bash -e enables errexit for subshell
bash -e -c 'false; echo "should not reach"'
echo "exit:$?"
### expect
exit:1
### end

### bash_e_errexit_success
# bash -e with successful commands runs normally
bash -e -c 'echo hello; echo world'
### expect
hello
world
### end

### bash_e_errexit_conditional
# bash -e doesn't exit on conditional failure
bash -e -c 'if false; then echo y; else echo n; fi; echo ok'
### expect
n
ok
### end

### bash_x_xtrace
# bash -x enables xtrace output
### bash_diff
bash -x -c 'echo hello' 2>/dev/null
### expect
hello
### end

### bash_u_nounset
# bash -u enables nounset checking
### bash_diff
bash -u -c 'echo $UNDEFINED_VAR_XYZ' 2>/dev/null
echo "exit:$?"
### expect
exit:1
### end

### bash_o_errexit
# bash -o errexit enables errexit
bash -o errexit -c 'false; echo "should not reach"'
echo "exit:$?"
### expect
exit:1
### end

### bash_o_pipefail
# bash -o pipefail enables pipefail
### bash_diff
bash -o pipefail -c 'false | true; echo "exit:$?"'
### expect
exit:1
### end

### bash_o_nounset
# bash -o nounset enables nounset
### bash_diff
bash -o nounset -c 'echo $UNDEFINED_XYZ' 2>/dev/null
echo "exit:$?"
### expect
exit:1
### end

### bash_combined_eu
# bash -eu combines errexit and nounset
bash -eu -c 'false; echo "nope"'
echo "exit:$?"
### expect
exit:1
### end

### bash_e_does_not_leak
# bash -e doesn't affect parent shell
bash -e -c 'false'
false
echo "still running"
### expect
still running
### end

### bash_o_invalid_option
# bash -o with invalid option returns error
bash -o invalidopt -c 'echo hi' 2>/dev/null
echo "exit:$?"
### expect
exit:2
### end

### bash_o_noglob
# bash -o noglob disables glob expansion in subshell
mkdir -p /tmp/bng_test
touch /tmp/bng_test/a /tmp/bng_test/b
bash -o noglob -c 'echo /tmp/bng_test/*'
### expect
/tmp/bng_test/*
### end

### bash_f_noglob
# bash -f disables glob expansion
mkdir -p /tmp/bf_test
touch /tmp/bf_test/x /tmp/bf_test/y
bash -f -c 'echo /tmp/bf_test/*'
### expect
/tmp/bf_test/*
### end

### shlvl_starts_at_one
# Cases end in `true` so real bash under the comparison harness's `bash -c`
# does not exec the last command in place (L-PROC-003).
echo "$SHLVL"
### expect
1
### end

### shlvl_child_shell_increments
bash -c 'echo "a=$SHLVL"; true'
bash -c 'bash -c "echo b=\$SHLVL; true"; true'
true
### expect
a=2
b=3
### end

### shlvl_child_shell_increments_from_unset
unset SHLVL
bash -c 'echo "a=$SHLVL"; true'
bash -c 'bash -c "echo b=\$SHLVL; true"; true'
### expect
a=1
b=2
### end

### shlvl_child_shell_increments_an_inherited_value
export SHLVL=3
bash -c 'echo "c=$SHLVL"; true'
bash -c 'echo "d=${SHLVL}"; bash -c "echo e=\$SHLVL; true"; true'
true
### expect
c=4
d=4
e=5
### end

### shlvl_non_numeric_restarts_at_one
SHLVL=abc bash -c 'echo "f=$SHLVL"; true'
### expect
f=1
### end

### shlvl_too_high_resets_to_one
SHLVL=999 bash -c 'echo "g=$SHLVL"; true'
### expect
g=1
### end

### shlvl_negative_becomes_zero
SHLVL=-5 bash -c 'echo "h=$SHLVL"; true'
true
### expect
h=0
### end

### shlvl_is_exported_to_the_child
bash -c 'env | grep "^SHLVL=" ; true'
true
### expect
SHLVL=2
### end

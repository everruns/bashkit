# Memory budget desync after subshell/command-substitution state restoration
# Regression tests for issue #993

### budget_accurate_after_command_substitutions
# Memory budget should not inflate after many command substitutions.
# After 100 command substitutions that create variables internally,
# the parent shell should still be able to create variables.
for i in $(seq 1 100); do
  x=$(echo val)
done
# If budget were inflated, this would silently fail
testvar="works"
echo "$testvar"
### expect
works
### end

### budget_enforced_after_subshell
# Memory budget should remain accurate after subshell execution.
# Variables created in subshell should not affect parent budget.
(
  for i in $(seq 1 50); do
    eval "sub_v$i=$i"
  done
)
# Parent should still be able to create variables
parentvar="ok"
echo "$parentvar"
### expect
ok
### end

### subshell_vars_do_not_leak_budget
# Creating and destroying variables in subshells should not
# accumulate phantom budget entries.
for i in $(seq 1 200); do
  (eval "tmp_$i=value")
done
result="success"
echo "$result"
### expect
success
### end

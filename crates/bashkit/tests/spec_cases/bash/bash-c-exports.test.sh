### bash_c_sees_exported_vars
# bash -c should inherit exported variables
export TEST_EXPORT_VAR="visible"
bash -c 'echo "$TEST_EXPORT_VAR"'
### expect
visible
### end

### bash_c_assigns_from_export
# bash -c can assign from exported vars
export TEST_ASSIGN_VAR="value"
bash -c 'x=$TEST_ASSIGN_VAR; echo "x=$x"'
### expect
x=value
### end

### bash_c_multiple_exports
# bash -c sees multiple exports
export A1=one A2=two
bash -c 'echo "$A1 $A2"'
### expect
one two
### end

### printenv_missing_var
### exit_code: 1
# printenv returns 1 for missing variable
printenv NONEXISTENT_VAR_XYZ_123
### expect
### end

### printenv_no_args_empty
### bash_diff: VFS env starts empty, printenv shows nothing
# printenv with no args on empty env
printenv | wc -l
### expect
0
### end

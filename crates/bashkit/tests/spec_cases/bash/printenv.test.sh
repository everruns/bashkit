### printenv_missing_var
### exit_code: 1
# printenv returns 1 for missing variable
printenv NONEXISTENT_VAR_XYZ_123
### expect
### end

### printenv_no_args_empty
### bash_diff: VFS env starts with only PWD exported
# printenv with no args on the startup environment
printenv | wc -l
### expect
1
### end

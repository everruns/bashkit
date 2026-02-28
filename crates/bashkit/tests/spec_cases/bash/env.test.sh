### env_no_args_empty
### bash_diff: VFS env starts empty
# env with no args on empty environment
env | wc -l
### expect
0
### end

### env_ignore_environment
### bash_diff: VFS env starts empty so -i is same as default
# env -i starts with empty environment
env -i | wc -l
### expect
0
### end

### env_set_vars
### bash_diff: VFS env does not support running commands
# env with NAME=VALUE prints specified vars
env FOO=bar BAZ=qux | grep -c "="
### expect
2
### end

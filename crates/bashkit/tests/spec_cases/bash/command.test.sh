### command_v_builtin
# command -v finds builtins
command -v echo
### expect
echo
### end

### command_v_not_found
# command -v returns 1 for unknown commands
### exit_code:1
command -v nonexistent_cmd_xyz
### expect
### end

### command_v_function
# command -v finds user functions
my_func() { echo hi; }
command -v my_func
### expect
my_func
### end

### command_V_builtin
# command -V describes builtins
command -V echo
### expect
echo is a shell builtin
### end

### command_V_function
# command -V describes functions
my_func() { echo hi; }
command -V my_func
### expect
my_func is a function
### end

### command_V_keyword
# command -V identifies keywords
command -V if
### expect
if is a shell keyword
### end

### command_run_builtin
# command runs builtins directly
command echo hello
### expect
hello
### end

### command_bypasses_function
# command bypasses function override
echo() { printf "OVERRIDE\n"; }
command echo real
### expect
real
### end

### command_v_keyword
# command -v finds shell keywords
command -v for
### expect
for
### end

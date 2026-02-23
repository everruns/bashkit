### declare_basic
# declare sets a variable
declare myvar=hello
echo "$myvar"
### expect
hello
### end

### declare_integer
# declare -i creates integer variable
declare -i num=42
echo "$num"
### expect
42
### end

### declare_integer_non_numeric
# declare -i with non-numeric defaults to 0
declare -i num=abc
echo "$num"
### expect
0
### end

### declare_readonly
# declare -r makes variable readonly
### bash_diff: bashkit stores readonly marker differently
declare -r RO=immutable
echo "$RO"
### expect
immutable
### end

### declare_export
# declare -x exports variable
### bash_diff: bashkit env model differs from real bash
declare -x MYENV=exported
echo "$MYENV"
### expect
exported
### end

### declare_array
# declare -a creates indexed array
declare -a arr
arr[0]=first
arr[1]=second
echo "${arr[0]} ${arr[1]}"
### expect
first second
### end

### declare_print_var
# declare -p prints variable declaration
myvar=hello
declare -p myvar
### expect
declare -- myvar="hello"
### end

### declare_print_not_found
### exit_code:1
### bash_diff: real bash outputs error to stderr; bashkit uses ExecResult::err
# declare -p for nonexistent variable
declare -p nonexistent_xyz
### expect
### end

### declare_no_value
# declare without value initializes empty
declare emptyvar
echo "val=$emptyvar"
### expect
val=
### end

### typeset_alias
# typeset is alias for declare
typeset myvar=hello
echo "$myvar"
### expect
hello
### end

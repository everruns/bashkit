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

### nameref_basic
# declare -n creates a name reference
x=hello
declare -n ref=x
echo "$ref"
### expect
hello
### end

### nameref_assign_through
# Assigning to nameref assigns to target variable
x=old
declare -n ref=x
ref=new
echo "$x"
### expect
new
### end

### nameref_chain
# Nameref can chain through another nameref
a=value
declare -n b=a
declare -n c=b
echo "$c"
### expect
value
### end

### nameref_in_function
# Nameref used to pass variable names to functions
set_via_ref() {
  declare -n ref=$1
  ref="set_by_function"
}
result=""
set_via_ref result
echo "$result"
### expect
set_by_function
### end

### nameref_read_unset
# Reading through nameref to unset variable returns empty
declare -n ref=nonexistent_var
echo "[$ref]"
### expect
[]
### end

### nameref_reassign_target
# Changing the target variable reflects through the nameref
x=first
declare -n ref=x
echo "$ref"
x=second
echo "$ref"
### expect
first
second
### end

### declare_lowercase
# declare -l converts value to lowercase
declare -l x=HELLO
echo "$x"
### expect
hello
### end

### declare_uppercase
# declare -u converts value to uppercase
declare -u x=hello
echo "$x"
### expect
HELLO
### end

### declare_lowercase_subsequent
# declare -l applies to subsequent assignments
declare -l x
x=WORLD
echo "$x"
### expect
world
### end

### declare_uppercase_subsequent
# declare -u applies to subsequent assignments
declare -u x
x=world
echo "$x"
### expect
WORLD
### end

### declare_lowercase_mixed
# declare -l handles mixed case
declare -l x=HeLLo_WoRLd
echo "$x"
### expect
hello_world
### end

### declare_uppercase_overrides_lowercase
# declare -u after -l overrides to uppercase
declare -l x=Hello
echo "$x"
declare -u x
x=Hello
echo "$x"
### expect
hello
HELLO
### end

### declare_case_in_function
# declare -l works in functions
toupper() {
  declare -u result="$1"
  echo "$result"
}
toupper "hello world"
### expect
HELLO WORLD
### end

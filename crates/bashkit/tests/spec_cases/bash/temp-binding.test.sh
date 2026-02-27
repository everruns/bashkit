# Temporary variable binding tests
# Inspired by Oils spec/temp-binding.test.sh
# https://github.com/oilshell/oil/blob/master/spec/temp-binding.test.sh

### temp_basic
# Temporary binding for command
X=original
X=temp echo done
echo X=$X
### expect
done
X=original
### end

### temp_in_env
# Temp binding visible in command's environment
X=original
X=override printenv X
echo X=$X
### expect
override
X=original
### end

### temp_multiple
# Multiple temp bindings
A=1 B=2 printenv A
A=1 B=2 printenv B
echo A=$A B=$B
### expect
1
2
A= B=
### end

### temp_with_builtin
# Temp binding with builtin command
IFS=: read a b c <<< "x:y:z"
echo "$a $b $c"
### expect
x y z
### end

### temp_empty_command
# Temp binding with no command persists
X=before
X=after
echo X=$X
### expect
X=after
### end

### temp_in_function
# Temp binding with function call
f() { echo "inside X=$X"; }
X=original
X=temp f
echo "after X=$X"
### expect
inside X=temp
after X=original
### end

### temp_ifs_for_read
# IFS temp binding for read
echo "a:b:c" | { IFS=: read x y z; echo "$x $y $z"; }
### expect
a b c
### end

### temp_overwrites_during_command
# Temp binding overwrites var during command only
X=original
show() { echo "X=$X"; }
X=temp show
echo "X=$X"
### expect
X=temp
X=original
### end

### temp_unset_var
# Temp binding on previously unset variable
unset TEMP_VAR_XYZ
TEMP_VAR_XYZ=hello printenv TEMP_VAR_XYZ
echo "after=${TEMP_VAR_XYZ:-unset}"
### expect
hello
after=unset
### end

### temp_export_behavior
# Temp binding makes var available in child env
X=exported bash -c 'echo X=$X'
### expect
X=exported
### end

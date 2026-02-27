# Subshell tests
# Inspired by Oils spec/subshell.test.sh
# https://github.com/oilshell/oil/blob/master/spec/subshell.test.sh

### subshell_exit_code
# Subshell exit code
( false; )
echo $?
### expect
1
### end

### subshell_with_redirects
# Subshell with redirects
( echo 1 ) > /tmp/bashkit_sub_a.txt
( echo 2 ) > /tmp/bashkit_sub_b.txt
( echo 3; ) > /tmp/bashkit_sub_c.txt
( echo 4; echo 5 ) > /tmp/bashkit_sub_d.txt
echo status=$?
cat /tmp/bashkit_sub_a.txt /tmp/bashkit_sub_b.txt /tmp/bashkit_sub_c.txt /tmp/bashkit_sub_d.txt
rm -f /tmp/bashkit_sub_a.txt /tmp/bashkit_sub_b.txt /tmp/bashkit_sub_c.txt /tmp/bashkit_sub_d.txt
### expect
status=0
1
2
3
4
5
### end

### subshell_var_isolation
# Variables set in subshell don't leak
X=original
( X=modified )
echo $X
### expect
original
### end

### subshell_function_isolation
# Functions defined in subshell don't leak
### skip: TODO function definitions in subshell leak to parent scope
( f() { echo inside; }; f )
f 2>/dev/null
echo status=$?
### expect
inside
status=127
### end

### subshell_nested
# Nested subshells
echo $(echo $(echo deep))
### expect
deep
### end

### subshell_exit_propagation
# Exit in subshell doesn't exit parent
( exit 42 )
echo "still running, status=$?"
### expect
still running, status=42
### end

### subshell_pipeline
# Subshell in pipeline
( echo hello; echo world ) | sort
### expect
hello
world
### end

### subshell_cd_isolation
# cd in subshell doesn't affect parent
### skip: TODO cd in subshell leaks to parent (VFS model)
original=$(pwd)
( cd / )
test "$(pwd)" = "$original" && echo isolated
### expect
isolated
### end

### subshell_traps_isolated
# Traps in subshell don't leak to parent
### skip: TODO trap in subshell not isolated
trap 'echo parent' EXIT
( trap 'echo child' EXIT )
trap - EXIT
echo done
### expect
child
done
### end

### subshell_brace_group
# Brace group is NOT a subshell
X=original
{ X=modified; }
echo $X
### expect
modified
### end

### subshell_command_sub_exit
# Command substitution exit code
result=$(exit 3)
echo "status=$?"
### expect
status=3
### end

### subshell_multiple_statements
# Multiple statements in subshell
(
  echo first
  echo second
  echo third
)
### expect
first
second
third
### end

### subshell_preserves_positional
# Positional params in subshell don't leak
### skip: TODO positional params in subshell leak to parent
set -- a b c
( set -- x y; echo "$@" )
echo "$@"
### expect
x y
a b c
### end

### recursive_function_command_subst
# Recursive function calls inside $() should work
factorial() {
  if (( $1 <= 1 )); then echo 1
  else echo $(( $1 * $(factorial $(($1 - 1))) ))
  fi
}
factorial 5
### expect
120
### end

### recursive_depth_3
# Recursive function with depth 3
f() {
  if (( $1 <= 0 )); then echo "base"
  else echo "depth=$1 $(f $(($1 - 1)))"
  fi
}
f 3
### expect
depth=3 depth=2 depth=1 base
### end

### recursive_cmdsub_var_isolation
# Variable mutations inside $() in arithmetic should not leak to parent
x=100
inner() {
  x=999
  echo 42
}
result=$(( $(inner) + x ))
echo "$result $x"
### expect
142 100
### end

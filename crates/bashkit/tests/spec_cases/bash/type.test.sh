### type_builtin
# type reports builtins
type echo
### expect
echo is a shell builtin
### end

### type_keyword
# type reports keywords
type if
### expect
if is a shell keyword
### end

### type_function
### bash_diff: real bash also prints function body
# type reports functions
myfunc() { echo hi; }
type myfunc
### expect
myfunc is a function
### end

### type_not_found
### exit_code:1
### bash_diff: real bash writes error to stderr not stdout
# type exits 1 for unknown command
type nonexistent_cmd_xyz
### expect
bash: type: nonexistent_cmd_xyz: not found
### end

### type_t_builtin
# type -t prints just the type word
type -t echo
### expect
builtin
### end

### type_t_keyword
# type -t for keyword
type -t for
### expect
keyword
### end

### type_t_function
# type -t for function
myfunc() { echo hi; }
type -t myfunc
### expect
function
### end

### type_t_not_found
### exit_code:1
# type -t prints nothing for unknown
type -t nonexistent_cmd_xyz
### expect
### end

### type_multiple
# type handles multiple names
type echo true
### expect
echo is a shell builtin
true is a shell builtin
### end

### type_a_builtin
### bash_diff: real bash also shows PATH entries for echo
# type -a shows all matches
type -a echo
### expect
echo is a shell builtin
### end

### which_builtin
### bash_diff: real which shows PATH, bashkit shows name
# which finds builtins
which echo
### expect
echo
### end

### which_not_found
### exit_code:1
# which exits 1 for unknown command
which nonexistent_cmd_xyz
### expect
### end

### which_multiple
### bash_diff: real which shows PATH, bashkit shows name
# which handles multiple names
which echo cat
### expect
echo
cat
### end

### which_function
### bash_diff: real which only searches PATH, not functions
# which finds functions
myfunc() { echo hi; }
which myfunc
### expect
myfunc
### end

### hash_noop
### bash_diff: real bash prints hash table contents
# hash is a no-op in sandboxed env
hash
echo "ok"
### expect
ok
### end

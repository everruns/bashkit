### bash_c_simple
# bash -c executes command string
bash -c 'echo hello'
### expect
hello
### end

### bash_c_multiple_commands
# bash -c with semicolon-separated commands
bash -c 'echo one; echo two'
### expect
one
two
### end

### sh_c_simple
# sh -c is equivalent to bash -c
sh -c 'echo world'
### expect
world
### end

### bash_c_exit_code
# Exit code propagates from bash -c
bash -c 'exit 42'
echo "exit: $?"
### expect
exit: 42
### end

### bash_c_positional_zero
# $0 in bash -c is the first arg after command
bash -c 'echo $0' myname
### expect
myname
### end

### bash_c_positional_args
# $1, $2 in bash -c are subsequent args
bash -c 'echo $1 $2' _ first second
### expect
first second
### end

### bash_n_syntax_ok
# bash -n checks syntax without executing
bash -n -c 'echo hello; echo world'
echo "status: $?"
### expect
status: 0
### end

### bash_n_syntax_error
### exit_code: 2
# bash -n reports syntax errors
bash -n -c 'echo; if'
### expect
### end

### bash_n_combined_flag
# -n can be combined with other flags
bash -ne -c 'echo test'
echo "did not execute"
### expect
did not execute
### end

### bash_version
### bash_diff: returns Bashkit version, not GNU bash
# --version shows sandbox version
bash --version | grep -q "Bashkit" && echo "has Bashkit"
bash --version | grep -q "sandboxed" && echo "is sandboxed"
### expect
has Bashkit
is sandboxed
### end

### bash_help
### bash_diff: returns Bashkit help, not GNU bash help
# --help shows usage
bash --help | head -1
### expect
Usage: bash [option] ... [file [argument] ...]
### end

### bash_piped_stdin
# echo script | bash executes from stdin
echo 'echo from pipe' | bash
### expect
from pipe
### end

### bash_nested
# Nested bash calls work
bash -c "bash -c 'echo nested'"
### expect
nested
### end

### bash_double_dash
# -- ends option processing
bash -- -c 'echo test'
### bash_diff: real bash would look for file named "-c"
### expect
### end

### bash_empty
# bash with no args and no stdin does nothing
bash
echo "done"
### expect
done
### end

### bash_variable_export
# Variables set in bash -c affect caller (shared interpreter)
### bash_diff: real bash runs subshell, vars don't propagate
bash -c 'FOO=bar'
echo "FOO=$FOO"
### expect
FOO=bar
### end

### bash_arithmetic
# Arithmetic works in bash -c
bash -c 'echo $((2 + 3))'
### expect
5
### end

### bash_for_loop
# Loops work in bash -c
bash -c 'for i in a b c; do echo $i; done'
### expect
a
b
c
### end

### bash_function_def
# Functions can be defined and called in bash -c
bash -c 'f() { echo "called"; }; f'
### expect
called
### end

### bash_c_no_arg
### exit_code: 2
# bash -c without argument is an error
bash -c
### expect
### end

### sh_version
### bash_diff: returns Bashkit version, not real sh
# sh --version also works
sh --version | grep -q "sandboxed sh" && echo "is sh"
### expect
is sh
### end

### bash_missing_file
### exit_code: 127
# bash with missing file returns 127
bash /nonexistent/file.sh
### expect
### end

### bash_c_syntax_error
### exit_code: 2
# Syntax error returns exit code 2
bash -c 'if then'
### expect
### end

### bash_all_positional
# $@ and $* work in bash -c
bash -c 'echo "all: $@"' _ one two three
### expect
all: one two three
### end

### bash_arg_count
# $# works in bash -c
bash -c 'echo "count: $#"' _ a b c d
### expect
count: 4
### end

### nested_param_expansion_in_quotes
# Parameter expansion inside double-quoted string
x=hello; echo "prefix_${x}_suffix"
### expect
prefix_hello_suffix
### end

### array_in_arithmetic
# Array element in arithmetic
arr=(10 20 30); echo $(( arr[1] + arr[2] ))
### expect
50
### end

### string_replace_with_slash
# String replacement where pattern contains slash
x="a/b/c"; echo "${x//\//|}"
### expect
a|b|c
### end

### heredoc_with_arithmetic
# Heredoc with arithmetic expansion
cat <<EOF
Result: $((2 + 3))
EOF
### expect
Result: 5
### end

### nested_subshell
### skip: parser fuel exhausted on triple-nested subshells (#669)
# Nested subshells
(echo a; (echo b; (echo c)))
### expect
a
b
c
### end

### subshell_variable_not_leak
# Variable from subshell should not leak
(x=secret); echo "${x:-empty}"
### expect
empty
### end

### pipe_to_while_read_count
# While read in pipeline
echo -e "a\nb\nc" | while read line; do echo "[$line]"; done
### expect
[a]
[b]
[c]
### end

### function_overwrite
# Function redefinition
f() { echo v1; }; f; f() { echo v2; }; f
### expect
v1
v2
### end

### unset_function
# Unset a function
f() { echo hi; }; f; unset -f f; f 2>/dev/null; echo $?
### expect
hi
127
### end

### array_negative_slice
# Array slice with negative offset
arr=(a b c d e); echo "${arr[@]: -2}"
### expect
d e
### end

### printf_width_precision
# Printf with width and precision
printf "%.3f\n" 3.14159
### expect
3.142
### end

### printf_octal
# Printf octal
printf '%o\n' 255
### expect
377
### end

### read_n_chars
### bash_diff: pipe creates subshell in real bash, read -n result lost; bashkit keeps it
# Read specific number of characters
echo "hello" | read -n 3 x; echo "$x"
### expect
hel
### end

### test_string_comparison
# String comparison in [[ ]]
[[ "abc" < "def" ]] && echo yes || echo no
### expect
yes
### end

### test_string_gt
# String greater than
[[ "xyz" > "abc" ]] && echo yes || echo no
### expect
yes
### end

### arithmetic_modulo
# Modulo operation
echo $(( 17 % 5 ))
### expect
2
### end

### arithmetic_logical_and
# Logical AND in arithmetic
echo $(( 1 && 1 )); echo $(( 1 && 0 ))
### expect
1
0
### end

### arithmetic_logical_or
# Logical OR in arithmetic
echo $(( 0 || 1 )); echo $(( 0 || 0 ))
### expect
1
0
### end

### arithmetic_logical_not
# Logical NOT
echo $(( !0 )); echo $(( !5 ))
### expect
1
0
### end

### arithmetic_bitwise_or
# Bitwise OR
echo $(( 0x0F | 0xF0 ))
### expect
255
### end

### arithmetic_bitwise_xor
# Bitwise XOR
echo $(( 0xFF ^ 0x0F ))
### expect
240
### end

### arithmetic_bitwise_not
# Bitwise NOT (complement)
echo $(( ~0 ))
### expect
-1
### end

### multiple_for_vars
# Iterate multiple items per line using printf join pattern
result=""; for i in a b c; do result="${result}${i} "; done; echo "${result% }"
### expect
a b c
### end

### case_with_variable_pattern
# Case with variable as pattern
pat="hello"; case "hello" in $pat) echo matched;; esac
### expect
matched
### end

### redirect_fd_read
### skip: exec N< file (input fd redirect) causes parse error (#669)
# Read from custom file descriptor
echo "from fd" > /tmp/fd_read_test; exec 4< /tmp/fd_read_test; read line <&4; exec 4<&-; echo "$line"
### expect
from fd
### end

### dev_null_redirect
# Redirect to /dev/null
echo "gone" > /dev/null; echo "visible"
### expect
visible
### end

### empty_command_substitution
# Empty command substitution
x=$(true); echo "[$x]"
### expect
[]
### end

### command_sub_strips_trailing_newlines
# Command sub strips trailing newlines
x=$(printf "hello\n\n\n"); echo "$x"
### expect
hello
### end

### while_break_n
# Break with level
for i in 1 2 3; do for j in a b c; do if [ "$j" = b ]; then break 2; fi; echo "$i$j"; done; done; echo done
### expect
1a
done
### end

### continue_n
# Continue with level
for i in 1 2; do for j in a b c; do if [ "$j" = b ]; then continue 2; fi; echo "$i$j"; done; done
### expect
1a
2a
### end

### return_from_sourced
# Return from sourced file
echo 'echo before; return; echo after' > /tmp/test_return.sh; source /tmp/test_return.sh; echo done
### expect
before
done
### end

### string_contains_newline
# Test string contains newline
x=$'hello\nworld'; [[ "$x" == *$'\n'* ]] && echo has_newline || echo no_newline
### expect
has_newline
### end

### numeric_string_compare
# Numeric comparison as strings in [[ ]]
[[ 9 > 10 ]] && echo string || echo numeric
### expect
string
### end

### empty_array
# Empty array behavior
arr=(); echo ${#arr[@]}
### expect
0
### end

### array_from_command
# Array from command output
arr=($(echo a b c d)); echo ${#arr[@]} ${arr[2]}
### expect
4 c
### end

### assoc_array_default
# Associative array default value
declare -A m; echo "${m[nokey]:-default}"
### expect
default
### end

### assoc_array_check_key
# Check if associative array key exists
declare -A m; m[foo]=bar; [[ -v m[foo] ]] && echo exists || echo missing
### expect
exists
### end

### assoc_array_unset_key
# Unset associative array key
declare -A m; m[a]=1; m[b]=2; unset m[a]; echo ${#m[@]}
### expect
1
### end

### declare_i_arithmetic
### bash_diff: declare -i does not auto-evaluate arithmetic in assignments (#664)
# Integer variable auto-evaluates — bash: "8\n8", bashkit: "3+5\n2 * 4"
declare -i x; x=3+5; echo $x; x="2 * 4"; echo $x
### expect
3+5
2 * 4
### end

### trap_debug
### bash_diff: DEBUG trap handler not implemented (counter stays 0) (#675)
# DEBUG trap — bash: "a\nb\n2", bashkit: "a\nb\n0"
count=0; trap '((count++))' DEBUG; echo a; echo b; trap - DEBUG; echo $count
### expect
a
b
0
### end

### nested_command_substitution_complex
# Complex nested command substitution
echo "$(echo "inner: $(echo deep)")"
### expect
inner: deep
### end

### double_bracket_negation
# Negation in [[ ]]
[[ ! 1 -eq 2 ]] && echo correct
### expect
correct
### end

### test_empty_string_in_condition
# Empty string test
x=""; if [ "$x" ]; then echo nonempty; else echo empty; fi
### expect
empty
### end

### while_false
# While false never executes
while false; do echo should_not; done; echo done
### expect
done
### end

### compound_command_redirect
# Compound command with redirect
{ echo hello; echo world; } > /tmp/compound_redir; cat /tmp/compound_redir
### expect
hello
world
### end

### for_loop_command_sub
# For loop over command substitution
for x in $(echo a b c); do echo $x; done
### expect
a
b
c
### end

### variable_with_spaces_in_quotes
# Variable with spaces preserved in quotes
x="hello   world   foo"; echo "$x"
### expect
hello   world   foo
### end

### arithmetic_nested_parens
# Nested parentheses in arithmetic
echo $(( ((2 + 3)) * ((4 - 1)) ))
### expect
15
### end

### string_replace_beginning
# Replace at beginning of string
x=hello; echo "${x/#h/H}"
### expect
Hello
### end

### string_replace_end
# Replace at end of string
x=hello; echo "${x/%o/O}"
### expect
hellO
### end

### subshell_pipe_exit
# Subshell in pipeline preserves exit code
(echo hello) | cat; echo $?
### expect
hello
0
### end

### multiple_assignments_in_one_line
# Temporary env vars for external command should not leak into shell scope
x=1 y=2 z=3 env 2>/dev/null | grep -c "^$" > /dev/null; [ -z "$x" ] && echo "no_leak" || echo "leaked"
### expect
no_leak
### end

### here_string_no_trailing_newline
# Herestring adds trailing newline
cat <<< "test" | wc -l
### expect
1
### end

### printf_zero_pad
# Printf zero-padded number
printf "%05d\n" 42
### expect
00042
### end

### printf_left_align
# Printf left-aligned
printf "%-10s|\n" hi
### expect
hi        |
### end

### printf_escape_n
# Printf with \n
printf "hello\nworld\n"
### expect
hello
world
### end

### echo_no_newline
# Echo without trailing newline
echo -n hello; echo " world"
### expect
hello world
### end

### test_integer_eq
# Integer equality
[ 42 -eq 42 ] && echo eq
### expect
eq
### end

### test_string_empty
# Test empty string with [ ]
[ "" ] && echo notempty || echo isempty
### expect
isempty
### end

### for_brace_expansion
# For loop with brace expansion
for i in {1..3}; do echo $i; done
### expect
1
2
3
### end

### variable_substitution_in_case
# Variable in case statement
cmd="start"; case $cmd in start) echo starting;; stop) echo stopping;; esac
### expect
starting
### end

### pipe_status_check
# Pipe with failing first command
false | echo hello; echo ${PIPESTATUS[0]}
### expect
hello
1
### end

### complex_string_building
# Build string incrementally with trimming
s=""; for i in 1 2 3; do s="${s}item$i "; done; s="${s% }"; echo "$s"
### expect
item1 item2 item3
### end

### set_positional_params
# Set and access positional parameters
set -- alpha beta gamma; echo $1; echo $3; echo $#
### expect
alpha
gamma
3
### end

### shift_all
# Shift through all positional params
set -- a b c; shift 2; echo "$@"
### expect
c
### end

### array_string_append
# String append to array element
arr=(hello); arr[0]+=" world"; echo "${arr[0]}"
### expect
hello world
### end

### unset_variable
# Unset variable
x=hello; unset x; echo "${x:-gone}"
### expect
gone
### end

### math_negative
# Negative number arithmetic
echo $(( -5 + 3 ))
### expect
-2
### end

### regex_no_match
# Regex that doesn't match
[[ "hello" =~ ^[0-9]+$ ]] && echo match || echo nomatch
### expect
nomatch
### end

### empty_heredoc
# Empty heredoc
cat <<EOF
EOF
### expect

### end

### array_assign_range
# Assign to array with arithmetic
for ((i=0; i<5; i++)); do arr[$i]=$((i*10)); done; echo ${arr[@]}
### expect
0 10 20 30 40
### end

### quoted_glob_no_expand
# Quoted glob should not expand
echo "*.txt"
### expect
*.txt
### end

### single_quoted_no_expand
# Single quotes prevent all expansion
echo '$HOME is "home" and `cmd`'
### expect
$HOME is "home" and `cmd`
### end

### command_exit_true_false
# Exit codes of true and false
true; echo $?; false; echo $?
### expect
0
1
### end

### nested_group_commands
# Nested group commands
{ { echo inner; }; echo outer; }
### expect
inner
outer
### end

### pipe_to_head
# Pipeline with head (SIGPIPE handling)
seq 1 100 | head -3
### expect
1
2
3
### end

### function_no_args
# Function called with no args
f() { echo "args: $#"; }; f
### expect
args: 0
### end

### read_default_variable
# Read with default variable REPLY
echo "hello" | { read; echo "$REPLY"; }
### expect
hello
### end

### complex_for_pattern
# For with complex word list
for f in file{1,2,3}.txt; do echo $f; done
### expect
file1.txt
file2.txt
file3.txt
### end

### chained_string_operations
### bash_diff: [![:space:]] character class in parameter expansion not supported (#677)
# Multiple string operations — bash: "[hello world  ]", bashkit: "[  hello world  ]"
x="  hello world  "; x="${x#"${x%%[![:space:]]*}"}"; echo "[$x]"
### expect
[  hello world  ]
### end

### multiline_if
# If statement across multiple lines
if true
then
  echo yes
fi
### expect
yes
### end

### multiline_for
# For loop across multiple lines
for i in 1 2 3
do
  echo $i
done
### expect
1
2
3
### end

### multiline_while
# While loop across multiple lines
i=0
while [ $i -lt 3 ]
do
  echo $i
  i=$((i+1))
done
### expect
0
1
2
### end

### multiline_case
# Case statement across multiple lines
x=b
case $x in
  a)
    echo alpha
    ;;
  b)
    echo beta
    ;;
esac
### expect
beta
### end

### multiline_function
# Function definition across multiple lines
greet() {
  echo "Hello, $1!"
}
greet World
### expect
Hello, World!
### end

### redirect_stderr_to_stdout
### bash_diff: bashkit captures stderr to stdout; real bash sends >&2 to terminal
# 2>&1 redirect
echo "out"; echo "err" >&2 2>&1
### expect
out
err
### end

### command_substitution_with_error
# Command substitution where command fails
x=$(cat /nonexistent/file 2>/dev/null); echo "got: [$x]"
### expect
got: []
### end

### while_read_with_field_sep
# While read with custom IFS
printf "a:1\nb:2\nc:3\n" | while IFS=: read key val; do echo "$key=$val"; done
### expect
a=1
b=2
c=3
### end

### nested_arithmetic_in_string
# Arithmetic inside string
x=5; echo "result is $((x * 2))"
### expect
result is 10
### end

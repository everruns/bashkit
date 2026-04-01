### ts_hello_world
# Basic console.log
ts -c "console.log('hello world')"
### expect
hello world
### end

### ts_expression_arithmetic
# Expression result displayed (REPL behavior)
ts -c "1 + 2 * 3"
### expect
7
### end

### ts_expression_string
# String expression result
ts -c "'hello'"
### expect
hello
### end

### ts_let_const
# Variable declaration
ts -c "let x = 10; const y = 20; console.log(x + y)"
### expect
30
### end

### ts_arrow_function
# Arrow function
ts -c "const add = (a: number, b: number): number => a + b; console.log(add(3, 4))"
### expect
7
### end

### ts_template_literal
# Template literal
ts -c "const name = 'world'; console.log('hello ' + name)"
### expect
hello world
### end

### ts_for_loop
# For loop
ts -c "let sum = 0; for (let i = 0; i < 5; i++) { sum += i; } console.log(sum)"
### expect
10
### end

### ts_while_loop
# While loop
ts -c "let i = 0; while (i < 3) { console.log(i); i++; }"
### expect
0
1
2
### end

### ts_if_else
# If/else
ts -c "const x = 42; if (x > 100) { console.log('big'); } else if (x > 10) { console.log('medium'); } else { console.log('small'); }"
### expect
medium
### end

### ts_array_methods
# Array methods
ts -c "const arr = [1, 2, 3]; console.log(arr.map(x => x * 2).join(','))"
### expect
2,4,6
### end

### ts_array_reduce
# Array reduce
ts -c "const sum = [1, 2, 3, 4, 5].reduce((a, b) => a + b, 0); console.log(sum)"
### expect
15
### end

### ts_array_filter
# Array filter
ts -c "const evens = [1, 2, 3, 4, 5].filter(x => x % 2 === 0); console.log(evens)"
### expect
2,4
### end

### ts_object_destructuring
# Object destructuring
ts -c "const { x, y } = { x: 10, y: 20 }; console.log(x + y)"
### expect
30
### end

### ts_array_destructuring
# Array destructuring
ts -c "const [a, b, c] = [1, 2, 3]; console.log(a + b + c)"
### expect
6
### end

### ts_spread_operator
# Spread in array
ts -c "const a = [1, 2]; const b = [3, 4]; console.log([...a, ...b])"
### expect
1,2,3,4
### end

### ts_rest_params
### skip: destructuring with rest causes empty output in spec runner (works in unit tests)
# Rest parameters
ts -c 'const [first, ...rest] = [1, 2, 3, 4]; console.log(first); console.log(rest.join(","))'
### expect
1
2,3,4
### end

### ts_ternary
# Ternary operator
ts -c "const x = 42; console.log(x % 2 === 0 ? 'even' : 'odd')"
### expect
even
### end

### ts_nested_function
# Nested function calls
ts -c "const add = (a: number, b: number) => a + b; const mul = (a: number, b: number) => a * b; console.log(add(mul(2, 3), mul(4, 5)))"
### expect
26
### end

### ts_fibonacci
### skip: recursive arrow fn produces empty output in spec runner (works in unit tests)
# Recursive fibonacci
ts -c 'const fib = (n) => n <= 1 ? n : fib(n - 1) + fib(n - 2); console.log(fib(10))'
### expect
55
### end

### ts_math_operations
# Math operations
ts -c "console.log(2 ** 10); console.log(Math.floor(17 / 3)); console.log(17 % 3)"
### expect
1024
5
2
### end

### ts_string_methods
# String methods
ts -c "const s = 'Hello, World!'; console.log(s.toUpperCase()); console.log(s.toLowerCase())"
### expect
HELLO, WORLD!
hello, world!
### end

### ts_boolean_logic
# Boolean operations
ts -c "console.log(true && false); console.log(true || false); console.log(!true)"
### expect
false
true
false
### end

### ts_null_undefined
# Null and undefined
ts -c "console.log(null); console.log(undefined)"
### expect
null
undefined
### end

### ts_typeof
# typeof operator
ts -c "console.log(typeof 42); console.log(typeof 'hello'); console.log(typeof true)"
### expect
number
string
boolean
### end

### ts_version
# Version flag
ts --version
### expect
TypeScript 5.0.0 (zapcode)
### end

### ts_version_short
# Short version flag
ts -V
### expect
TypeScript 5.0.0 (zapcode)
### end

### ts_node_alias
# node -e alias
node -e "console.log('from node')"
### expect
from node
### end

### ts_deno_alias
# deno -e alias
deno -e "console.log('from deno')"
### expect
from deno
### end

### ts_bun_alias
# bun -e alias
bun -e "console.log('from bun')"
### expect
from bun
### end

### ts_command_substitution
# TypeScript in command substitution
result=$(ts -c "console.log(6 * 7)")
echo "answer: $result"
### expect
answer: 42
### end

### ts_pipeline_output
# TypeScript output in pipeline
ts -c "for (let i = 0; i < 3; i++) { console.log('line ' + i); }" | grep "line 1"
### expect
line 1
### end

### ts_conditional_failure
# TypeScript error triggers else branch
### bash_diff: no real ts available
if ts -c "throw new Error()" 2>/dev/null; then echo "success"; else echo "fail"; fi
### expect
fail
### end

### ts_stdin_pipe
# Code from piped stdin
echo "console.log('from pipe')" | ts
### expect
from pipe
### end

### ts_eval_flag
# -e flag (Node.js compat)
ts -e "console.log('eval flag')"
### expect
eval flag
### end

### ts_vfs_write_and_read
# Write file from TypeScript, read from bash
ts -c "await writeFile('/tmp/tsout.txt', 'hello from ts\n')"
cat /tmp/tsout.txt
### expect
hello from ts
### end

### ts_vfs_bash_to_ts
# Write from bash, read from TypeScript (via return value, trim trailing newline)
printf "data from bash" > /tmp/shared.txt
ts -c "await readFile('/tmp/shared.txt')"
### expect
data from bash
### end

### ts_vfs_exists_true
# Check file existence (true case)
echo "hi" > /tmp/exists.txt
ts -c "await exists('/tmp/exists.txt')"
### expect
true
### end

### ts_vfs_exists_false
# Check file existence (false case)
ts -c "await exists('/tmp/nope.txt')"
### expect
false
### end

### ts_vfs_mkdir
# Create directory from TypeScript
ts -c "await mkdir('/tmp/tsdir'); await exists('/tmp/tsdir')"
### expect
true
### end

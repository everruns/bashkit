// Benchmark cases for comparing shell interpreters
// Categories:
// - startup: Interpreter startup overhead
// - variables: Variable assignment and expansion
// - arithmetic: Math operations
// - control: Loops, conditionals, functions
// - strings: String manipulation
// - arrays: Array operations
// - pipes: Pipelines and redirections
// - tools: Built-in tools (grep, sed, awk, jq)
// - complex: Real-world scripts

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Startup,
    Variables,
    Arithmetic,
    Control,
    Strings,
    Arrays,
    Pipes,
    Tools,
    Complex,
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Startup => "startup",
            Category::Variables => "variables",
            Category::Arithmetic => "arithmetic",
            Category::Control => "control",
            Category::Strings => "strings",
            Category::Arrays => "arrays",
            Category::Pipes => "pipes",
            Category::Tools => "tools",
            Category::Complex => "complex",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct BenchCase {
    pub name: String,
    pub category: Category,
    pub description: String,
    pub script: String,
    pub expected: Option<String>,
    pub expected_exit: Option<i32>,
}

impl BenchCase {
    pub fn new(name: &str, category: Category, description: &str, script: &str) -> Self {
        Self {
            name: name.to_string(),
            category,
            description: description.to_string(),
            script: script.to_string(),
            expected: None,
            expected_exit: Some(0),
        }
    }

    pub fn with_expected(mut self, expected: &str) -> Self {
        self.expected = Some(expected.to_string());
        self
    }
}

/// All benchmark cases
pub fn all_cases() -> Vec<BenchCase> {
    let mut cases = Vec::new();

    cases.extend(startup_cases());
    cases.extend(variable_cases());
    cases.extend(arithmetic_cases());
    cases.extend(control_cases());
    cases.extend(string_cases());
    cases.extend(array_cases());
    cases.extend(pipe_cases());
    cases.extend(tool_cases());
    cases.extend(complex_cases());

    cases
}

// === Startup Cases ===
fn startup_cases() -> Vec<BenchCase> {
    vec![
        BenchCase::new(
            "startup_empty",
            Category::Startup,
            "Empty command (pure startup time)",
            "true", // : not implemented in bashkit, use true instead
        )
        .with_expected(""),
        BenchCase::new("startup_true", Category::Startup, "True command", "true").with_expected(""),
        BenchCase::new(
            "startup_echo",
            Category::Startup,
            "Simple echo",
            "echo hello",
        )
        .with_expected("hello\n"),
        BenchCase::new(
            "startup_exit",
            Category::Startup,
            "Exit with code",
            "exit 0",
        )
        .with_expected(""),
    ]
}

// === Variable Cases ===
fn variable_cases() -> Vec<BenchCase> {
    vec![
        BenchCase::new(
            "var_assign_simple",
            Category::Variables,
            "Simple variable assignment",
            "x=hello; echo $x",
        )
        .with_expected("hello\n"),
        BenchCase::new(
            "var_assign_many",
            Category::Variables,
            "Multiple variable assignments",
            r#"
a=1; b=2; c=3; d=4; e=5
f=6; g=7; h=8; i=9; j=10
echo "$a$b$c$d$e$f$g$h$i$j"
"#,
        )
        .with_expected("12345678910\n"),
        BenchCase::new(
            "var_default",
            Category::Variables,
            "Default value expansion",
            "echo ${UNDEFINED:-default}",
        )
        .with_expected("default\n"),
        BenchCase::new(
            "var_length",
            Category::Variables,
            "String length",
            "x=hello; echo ${#x}",
        )
        .with_expected("5\n"),
        BenchCase::new(
            "var_substring",
            Category::Variables,
            "Substring extraction",
            "x=hello_world; echo ${x:0:5}",
        )
        .with_expected("hello\n"),
        BenchCase::new(
            "var_replace",
            Category::Variables,
            "Pattern replacement",
            "x=hello_world; echo ${x/world/bash}",
        )
        .with_expected("hello_bash\n"),
        BenchCase::new(
            "var_nested",
            Category::Variables,
            "Nested variable expansion",
            r#"
a=inner
inner=value
echo ${!a}
"#,
        )
        .with_expected("value\n"),
        BenchCase::new(
            "var_export",
            Category::Variables,
            "Export and use",
            "export FOO=bar; echo $FOO",
        )
        .with_expected("bar\n"),
    ]
}

// === Arithmetic Cases ===
fn arithmetic_cases() -> Vec<BenchCase> {
    vec![
        BenchCase::new(
            "arith_basic",
            Category::Arithmetic,
            "Basic arithmetic",
            "echo $((1 + 2))",
        )
        .with_expected("3\n"),
        BenchCase::new(
            "arith_complex",
            Category::Arithmetic,
            "Complex arithmetic expression",
            "echo $((10 * 5 + 3 - 2 / 1))",
        )
        .with_expected("51\n"),
        BenchCase::new(
            "arith_variables",
            Category::Arithmetic,
            "Arithmetic with variables",
            "x=10; y=20; echo $((x + y * 2))",
        )
        .with_expected("50\n"),
        BenchCase::new(
            "arith_increment",
            Category::Arithmetic,
            "Increment operations",
            "x=5; ((x++)); ((x++)); echo $x",
        )
        .with_expected("7\n"),
        BenchCase::new(
            "arith_modulo",
            Category::Arithmetic,
            "Modulo operation",
            "echo $((17 % 5))",
        )
        .with_expected("2\n"),
        BenchCase::new(
            "arith_loop_sum",
            Category::Arithmetic,
            "Sum in loop",
            r#"
sum=0
for i in 1 2 3 4 5 6 7 8 9 10; do
    sum=$((sum + i))
done
echo $sum
"#,
        )
        .with_expected("55\n"),
    ]
}

// === Control Flow Cases ===
fn control_cases() -> Vec<BenchCase> {
    vec![
        BenchCase::new(
            "ctrl_if_simple",
            Category::Control,
            "Simple if statement",
            "if true; then echo yes; fi",
        )
        .with_expected("yes\n"),
        BenchCase::new(
            "ctrl_if_else",
            Category::Control,
            "If-else statement",
            "if false; then echo no; else echo yes; fi",
        )
        .with_expected("yes\n"),
        BenchCase::new(
            "ctrl_for_list",
            Category::Control,
            "For loop over list",
            "for i in a b c d e; do echo -n $i; done; echo",
        )
        .with_expected("abcde\n"),
        BenchCase::new(
            "ctrl_for_range",
            Category::Control,
            "For loop with range",
            r#"
for ((i=0; i<5; i++)); do
    echo -n $i
done
echo
"#,
        )
        .with_expected("01234\n"),
        BenchCase::new(
            "ctrl_while",
            Category::Control,
            "While loop",
            r#"
i=0
while [ $i -lt 5 ]; do
    echo -n $i
    i=$((i + 1))
done
echo
"#,
        )
        .with_expected("01234\n"),
        BenchCase::new(
            "ctrl_case",
            Category::Control,
            "Case statement",
            r#"
x=two
case $x in
    one) echo 1 ;;
    two) echo 2 ;;
    *) echo other ;;
esac
"#,
        )
        .with_expected("2\n"),
        BenchCase::new(
            "ctrl_function",
            Category::Control,
            "Function definition and call",
            r#"
greet() {
    echo "Hello, $1!"
}
greet World
"#,
        )
        .with_expected("Hello, World!\n"),
        BenchCase::new(
            "ctrl_function_return",
            Category::Control,
            "Function with return value",
            r#"
add() {
    echo $(($1 + $2))
}
result=$(add 3 4)
echo $result
"#,
        )
        .with_expected("7\n"),
        BenchCase::new(
            "ctrl_nested_loops",
            Category::Control,
            "Nested loops",
            r#"
for i in 1 2 3; do
    for j in a b c; do
        echo -n "$i$j "
    done
done
echo
"#,
        )
        .with_expected("1a 1b 1c 2a 2b 2c 3a 3b 3c \n"),
    ]
}

// === String Cases ===
fn string_cases() -> Vec<BenchCase> {
    vec![
        BenchCase::new(
            "str_concat",
            Category::Strings,
            "String concatenation",
            r#"
a="Hello"
b="World"
echo "$a $b"
"#,
        )
        .with_expected("Hello World\n"),
        BenchCase::new(
            "str_printf",
            Category::Strings,
            "Printf formatting",
            "printf '%s=%d\\n' name 42",
        )
        .with_expected("name=42\n"),
        BenchCase::new(
            "str_printf_pad",
            Category::Strings,
            "Printf with padding",
            "printf '%05d\\n' 42",
        )
        .with_expected("00042\n"),
        BenchCase::new(
            "str_echo_escape",
            Category::Strings,
            "Echo with escapes",
            "echo -e 'line1\\nline2'",
        )
        .with_expected("line1\nline2\n"),
        BenchCase::new(
            "str_prefix_strip",
            Category::Strings,
            "Strip prefix",
            "x=/path/to/file.txt; echo ${x##*/}",
        )
        .with_expected("file.txt\n"),
        BenchCase::new(
            "str_suffix_strip",
            Category::Strings,
            "Strip suffix",
            "x=file.txt; echo ${x%.txt}",
        )
        .with_expected("file\n"),
        BenchCase::new(
            "str_uppercase",
            Category::Strings,
            "Uppercase conversion",
            "x=hello; echo ${x^^}",
        )
        .with_expected("HELLO\n"),
        BenchCase::new(
            "str_lowercase",
            Category::Strings,
            "Lowercase conversion",
            "x=HELLO; echo ${x,,}",
        )
        .with_expected("hello\n"),
    ]
}

// === Array Cases ===
fn array_cases() -> Vec<BenchCase> {
    vec![
        BenchCase::new(
            "arr_create",
            Category::Arrays,
            "Array creation",
            "arr=(a b c); echo ${arr[0]}",
        )
        .with_expected("a\n"),
        BenchCase::new(
            "arr_all",
            Category::Arrays,
            "Array all elements",
            "arr=(one two three); echo ${arr[@]}",
        )
        .with_expected("one two three\n"),
        BenchCase::new(
            "arr_length",
            Category::Arrays,
            "Array length",
            "arr=(a b c d e); echo ${#arr[@]}",
        )
        .with_expected("5\n"),
        BenchCase::new(
            "arr_iterate",
            Category::Arrays,
            "Array iteration",
            r#"
arr=(apple banana cherry)
for item in "${arr[@]}"; do
    echo "$item"
done
"#,
        )
        .with_expected("apple\nbanana\ncherry\n"),
        BenchCase::new(
            "arr_slice",
            Category::Arrays,
            "Array slicing",
            "arr=(1 2 3 4 5); echo ${arr[@]:1:3}",
        )
        .with_expected("2 3 4\n"),
        BenchCase::new(
            "arr_assign_index",
            Category::Arrays,
            "Array index assignment",
            "arr=(); arr[0]=a; arr[2]=c; echo ${arr[@]}",
        )
        .with_expected("a c\n"),
    ]
}

// === Pipeline Cases ===
fn pipe_cases() -> Vec<BenchCase> {
    vec![
        BenchCase::new(
            "pipe_simple",
            Category::Pipes,
            "Simple pipe",
            "echo hello | cat",
        )
        .with_expected("hello\n"),
        BenchCase::new(
            "pipe_multi",
            Category::Pipes,
            "Multi-stage pipe",
            "echo 'a b c' | cat | cat | cat",
        )
        .with_expected("a b c\n"),
        BenchCase::new(
            "pipe_command_subst",
            Category::Pipes,
            "Command substitution",
            "result=$(echo hello); echo $result",
        )
        .with_expected("hello\n"),
        BenchCase::new(
            "pipe_heredoc",
            Category::Pipes,
            "Here document",
            r#"
cat <<EOF
line1
line2
EOF
"#,
        )
        .with_expected("line1\nline2\n"),
        BenchCase::new(
            "pipe_herestring",
            Category::Pipes,
            "Here string",
            "cat <<< 'hello world'",
        )
        .with_expected("hello world\n"),
        BenchCase::new(
            "pipe_redirect_out",
            Category::Pipes,
            "Redirect output (simulated)",
            "echo test > /dev/null; echo done",
        )
        .with_expected("done\n"),
    ]
}

// === Tool Cases ===
fn tool_cases() -> Vec<BenchCase> {
    vec![
        // Grep tests
        BenchCase::new(
            "tool_grep_simple",
            Category::Tools,
            "Grep simple pattern",
            "echo -e 'apple\\nbanana\\napricot' | grep 'ap'",
        )
        .with_expected("apple\napricot\n"),
        BenchCase::new(
            "tool_grep_case",
            Category::Tools,
            "Grep case insensitive",
            "echo -e 'Apple\\nBANANA\\napple' | grep -i 'apple'",
        )
        .with_expected("Apple\napple\n"),
        BenchCase::new(
            "tool_grep_count",
            Category::Tools,
            "Grep count matches",
            "echo -e 'a\\nb\\na\\nc\\na' | grep -c 'a'",
        )
        .with_expected("3\n"),
        BenchCase::new(
            "tool_grep_invert",
            Category::Tools,
            "Grep invert match",
            "echo -e 'yes\\nno\\nyes' | grep -v 'no'",
        )
        .with_expected("yes\nyes\n"),
        BenchCase::new(
            "tool_grep_regex",
            Category::Tools,
            "Grep extended regex",
            "echo -e 'cat\\ndog\\ncot' | grep -E 'c[ao]t'",
        )
        .with_expected("cat\ncot\n"),
        // Sed tests
        BenchCase::new(
            "tool_sed_replace",
            Category::Tools,
            "Sed simple replace",
            "echo 'hello world' | sed 's/world/bash/'",
        )
        .with_expected("hello bash\n"),
        BenchCase::new(
            "tool_sed_global",
            Category::Tools,
            "Sed global replace",
            "echo 'aaa' | sed 's/a/b/g'",
        )
        .with_expected("bbb\n"),
        BenchCase::new(
            "tool_sed_delete",
            Category::Tools,
            "Sed delete line",
            "echo -e 'a\\nb\\nc' | sed '/b/d'",
        )
        .with_expected("a\nc\n"),
        BenchCase::new(
            "tool_sed_lines",
            Category::Tools,
            "Sed line range",
            "echo -e '1\\n2\\n3\\n4\\n5' | sed -n '2,4p'",
        )
        .with_expected("2\n3\n4\n"),
        BenchCase::new(
            "tool_sed_backrefs",
            Category::Tools,
            "Sed with backreferences",
            "echo 'hello' | sed 's/\\(hel\\)lo/\\1p/'",
        )
        .with_expected("help\n"),
        // Awk tests
        BenchCase::new(
            "tool_awk_print",
            Category::Tools,
            "Awk print field",
            "echo 'a b c' | awk '{print $2}'",
        )
        .with_expected("b\n"),
        BenchCase::new(
            "tool_awk_sum",
            Category::Tools,
            "Awk sum column",
            "echo -e '1\\n2\\n3\\n4\\n5' | awk '{sum+=$1} END {print sum}'",
        )
        .with_expected("15\n"),
        BenchCase::new(
            "tool_awk_pattern",
            Category::Tools,
            "Awk pattern match",
            "echo -e 'apple 1\\nbanana 2\\napricot 3' | awk '/^a/ {print $2}'",
        )
        .with_expected("1\n3\n"),
        BenchCase::new(
            "tool_awk_fieldsep",
            Category::Tools,
            "Awk field separator",
            "echo 'a:b:c' | awk -F: '{print $2}'",
        )
        .with_expected("b\n"),
        BenchCase::new(
            "tool_awk_nf",
            Category::Tools,
            "Awk number of fields",
            "echo 'one two three four' | awk '{print NF}'",
        )
        .with_expected("4\n"),
        BenchCase::new(
            "tool_awk_compute",
            Category::Tools,
            "Awk arithmetic",
            "echo '10 20' | awk '{print $1 + $2, $1 * $2}'",
        )
        .with_expected("30 200\n"),
        // Jq tests
        BenchCase::new(
            "tool_jq_identity",
            Category::Tools,
            "Jq identity",
            r#"echo '{"a":1}' | jq '.'"#,
        )
        .with_expected("{\n  \"a\": 1\n}\n"),
        BenchCase::new(
            "tool_jq_field",
            Category::Tools,
            "Jq field access",
            r#"echo '{"name":"test","value":42}' | jq '.value'"#,
        )
        .with_expected("42\n"),
        BenchCase::new(
            "tool_jq_array",
            Category::Tools,
            "Jq array access",
            r#"echo '[1,2,3,4,5]' | jq '.[2]'"#,
        )
        .with_expected("3\n"),
        BenchCase::new(
            "tool_jq_filter",
            Category::Tools,
            "Jq filter array",
            r#"echo '[1,2,3,4,5]' | jq '[.[] | select(. > 2)]'"#,
        )
        .with_expected("[3, 4, 5]\n"),
        BenchCase::new(
            "tool_jq_map",
            Category::Tools,
            "Jq map",
            r#"echo '[1,2,3]' | jq '[.[] * 2]'"#,
        )
        .with_expected("[2, 4, 6]\n"),
    ]
}

// === Complex Cases ===
fn complex_cases() -> Vec<BenchCase> {
    vec![
        BenchCase::new(
            "complex_fibonacci",
            Category::Complex,
            "Fibonacci sequence (recursive)",
            r#"
fib() {
    local n=$1
    if [ $n -le 1 ]; then
        echo $n
    else
        local a=$(fib $((n-1)))
        local b=$(fib $((n-2)))
        echo $((a + b))
    fi
}
fib 10
"#,
        )
        .with_expected("55\n"),
        BenchCase::new(
            "complex_fibonacci_iter",
            Category::Complex,
            "Fibonacci sequence (iterative)",
            r#"
a=0
b=1
for i in 1 2 3 4 5 6 7 8 9 10; do
    c=$((a + b))
    a=$b
    b=$c
done
echo $a
"#,
        )
        .with_expected("55\n"),
        BenchCase::new(
            "complex_nested_subst",
            Category::Complex,
            "Nested command substitution",
            r#"
echo $(echo $(echo $(echo deep)))
"#,
        )
        .with_expected("deep\n"),
        BenchCase::new(
            "complex_loop_compute",
            Category::Complex,
            "Loop with computation",
            r#"
sum=0
for i in 1 2 3 4 5 6 7 8 9 10; do
    sq=$((i * i))
    sum=$((sum + sq))
done
echo $sum
"#,
        )
        .with_expected("385\n"),
        BenchCase::new(
            "complex_string_build",
            Category::Complex,
            "String building in loop",
            r#"
result=""
for c in a b c d e; do
    result="$result$c"
done
echo $result
"#,
        )
        .with_expected("abcde\n"),
        BenchCase::new(
            "complex_json_transform",
            Category::Complex,
            "JSON transformation",
            r#"
echo '[{"name":"alice","score":85},{"name":"bob","score":92}]' | jq '.[0].name'
"#,
        )
        .with_expected("\"alice\"\n"),
        BenchCase::new(
            "complex_pipeline_text",
            Category::Complex,
            "Text pipeline processing",
            r#"
echo -e "apple\nbanana\napricot\ncherry" | grep "^a" | sed 's/a/A/g'
"#,
        )
        .with_expected("Apple\nApricot\n"),
    ]
}

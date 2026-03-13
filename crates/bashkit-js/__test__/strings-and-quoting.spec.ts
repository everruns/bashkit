import test from "ava";
import { Bash } from "../wrapper.js";

// ============================================================================
// Quoting
// ============================================================================

test("single quotes preserve literal", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("echo '$HOME'").stdout.trim(), "$HOME");
});

test("double quotes expand variables", (t) => {
  const bash = new Bash();
  bash.executeSync("X=world");
  t.is(bash.executeSync('echo "hello $X"').stdout.trim(), "hello world");
});

test("double quotes preserve spaces", (t) => {
  const bash = new Bash();
  bash.executeSync('X="hello   world"');
  t.is(bash.executeSync('echo "$X"').stdout.trim(), "hello   world");
});

test("backslash escaping in double quotes", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync('echo "a\\$b"').stdout.trim(), "a$b");
});

test("nested command substitution in quotes", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync('echo "count: $(echo 42)"').stdout.trim(),
    "count: 42"
  );
});

// ============================================================================
// Here documents
// ============================================================================

test("heredoc basic", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`cat <<EOF
hello world
EOF`);
  t.is(r.stdout.trim(), "hello world");
});

test("heredoc with variable expansion", (t) => {
  const bash = new Bash();
  bash.executeSync("NAME=alice");
  const r = bash.executeSync(`cat <<EOF
hello $NAME
EOF`);
  t.is(r.stdout.trim(), "hello alice");
});

test("heredoc quoted delimiter suppresses expansion", (t) => {
  const bash = new Bash();
  bash.executeSync("NAME=alice");
  const r = bash.executeSync(`cat <<'EOF'
hello $NAME
EOF`);
  t.is(r.stdout.trim(), "hello $NAME");
});

// ============================================================================
// String operations
// ============================================================================

test("string concatenation", (t) => {
  const bash = new Bash();
  bash.executeSync('A=hello; B=world');
  t.is(bash.executeSync('echo "${A}${B}"').stdout.trim(), "helloworld");
});

test("string replacement", (t) => {
  const bash = new Bash();
  bash.executeSync("S='hello world hello'");
  // First occurrence
  t.is(bash.executeSync('echo "${S/hello/bye}"').stdout.trim(), "bye world hello");
});

test("string replacement global", (t) => {
  const bash = new Bash();
  bash.executeSync("S='hello world hello'");
  // All occurrences
  t.is(bash.executeSync('echo "${S//hello/bye}"').stdout.trim(), "bye world bye");
});

test("uppercase conversion", (t) => {
  const bash = new Bash();
  bash.executeSync("S=hello");
  t.is(bash.executeSync('echo "${S^^}"').stdout.trim(), "HELLO");
});

test("lowercase conversion", (t) => {
  const bash = new Bash();
  bash.executeSync("S=HELLO");
  t.is(bash.executeSync('echo "${S,,}"').stdout.trim(), "hello");
});

// ============================================================================
// Arrays
// ============================================================================

test("array declaration and access", (t) => {
  const bash = new Bash();
  bash.executeSync("ARR=(apple banana cherry)");
  t.is(bash.executeSync('echo "${ARR[0]}"').stdout.trim(), "apple");
  t.is(bash.executeSync('echo "${ARR[2]}"').stdout.trim(), "cherry");
});

test("array length", (t) => {
  const bash = new Bash();
  bash.executeSync("ARR=(a b c d)");
  t.is(bash.executeSync('echo "${#ARR[@]}"').stdout.trim(), "4");
});

test("array all elements", (t) => {
  const bash = new Bash();
  bash.executeSync("ARR=(x y z)");
  t.is(bash.executeSync('echo "${ARR[@]}"').stdout.trim(), "x y z");
});

test("array append", (t) => {
  const bash = new Bash();
  bash.executeSync("ARR=(a b)");
  bash.executeSync("ARR+=(c)");
  t.is(bash.executeSync('echo "${ARR[@]}"').stdout.trim(), "a b c");
});

test("array in for loop", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    FRUITS=(apple banana cherry)
    for f in "\${FRUITS[@]}"; do
      echo "$f"
    done
  `);
  t.is(r.stdout.trim(), "apple\nbanana\ncherry");
});

// ============================================================================
// Special characters and edge cases
// ============================================================================

test("empty string variable", (t) => {
  const bash = new Bash();
  bash.executeSync('X=""');
  t.is(bash.executeSync('echo "[$X]"').stdout.trim(), "[]");
});

test("newlines in variable", (t) => {
  const bash = new Bash();
  bash.executeSync('X="line1\nline2"');
  const r = bash.executeSync('echo -e "$X"');
  t.true(r.stdout.includes("line1"));
  t.true(r.stdout.includes("line2"));
});

test("tab character", (t) => {
  const bash = new Bash();
  t.true(bash.executeSync("echo -e 'a\\tb'").stdout.includes("\t"));
});

test("semicolon separates commands", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo a; echo b");
  t.is(r.stdout.trim(), "a\nb");
});

test("long string handling", (t) => {
  const bash = new Bash();
  const long = "x".repeat(10000);
  bash.executeSync(`X="${long}"`);
  const r = bash.executeSync('echo ${#X}');
  t.is(r.stdout.trim(), "10000");
});

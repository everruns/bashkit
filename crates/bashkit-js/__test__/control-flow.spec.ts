import test from "ava";
import { Bash } from "../wrapper.js";

// ============================================================================
// Conditionals
// ============================================================================

test("if/then/fi true branch", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('if true; then echo "yes"; fi');
  t.is(r.stdout.trim(), "yes");
});

test("if/else false branch", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('if false; then echo "yes"; else echo "no"; fi');
  t.is(r.stdout.trim(), "no");
});

test("if/elif/else chain", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    X=2
    if [ "$X" = "1" ]; then echo one
    elif [ "$X" = "2" ]; then echo two
    else echo other
    fi
  `);
  t.is(r.stdout.trim(), "two");
});

test("nested if", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    A=1; B=2
    if [ "$A" = "1" ]; then
      if [ "$B" = "2" ]; then
        echo both
      fi
    fi
  `);
  t.is(r.stdout.trim(), "both");
});

test("test string equality", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync('[ "abc" = "abc" ] && echo match').stdout.trim(), "match");
  t.not(bash.executeSync('[ "abc" = "xyz" ] && echo match').exitCode, 0);
});

test("test numeric comparison", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("[ 5 -gt 3 ] && echo yes").stdout.trim(), "yes");
  t.is(bash.executeSync("[ 3 -lt 5 ] && echo yes").stdout.trim(), "yes");
  t.is(bash.executeSync("[ 5 -eq 5 ] && echo yes").stdout.trim(), "yes");
  t.not(bash.executeSync("[ 5 -lt 3 ]").exitCode, 0);
});

test("logical AND (&&)", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("true && echo yes").stdout.trim(), "yes");
  t.is(bash.executeSync("false && echo yes").stdout, "");
});

test("logical OR (||)", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("false || echo fallback").stdout.trim(), "fallback");
  t.is(bash.executeSync("true || echo fallback").stdout.trim(), "");
});

test("AND + OR chaining", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync("false && echo a || echo b").stdout.trim(),
    "b"
  );
});

// ============================================================================
// Loops
// ============================================================================

test("for loop over values", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("for i in a b c; do echo $i; done");
  t.is(r.stdout.trim(), "a\nb\nc");
});

test("for loop with seq-style range", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("for i in 1 2 3 4 5; do echo -n $i; done");
  t.is(r.stdout, "12345");
});

test("for loop with command substitution", (t) => {
  const bash = new Bash();
  bash.executeSync('echo -e "x\\ny\\nz" > /tmp/items.txt');
  const r = bash.executeSync("for i in $(cat /tmp/items.txt); do echo -n $i; done");
  t.is(r.stdout, "xyz");
});

test("while loop", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    i=0
    while [ $i -lt 3 ]; do
      echo $i
      i=$((i + 1))
    done
  `);
  t.is(r.stdout.trim(), "0\n1\n2");
});

test("until loop", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    i=0
    until [ $i -ge 3 ]; do
      echo $i
      i=$((i + 1))
    done
  `);
  t.is(r.stdout.trim(), "0\n1\n2");
});

test("break in loop", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    for i in 1 2 3 4 5; do
      if [ $i -eq 3 ]; then break; fi
      echo $i
    done
  `);
  t.is(r.stdout.trim(), "1\n2");
});

test("continue in loop", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    for i in 1 2 3 4 5; do
      if [ $i -eq 3 ]; then continue; fi
      echo $i
    done
  `);
  t.is(r.stdout.trim(), "1\n2\n4\n5");
});

// ============================================================================
// Case statement
// ============================================================================

test("case statement match", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    X=hello
    case "$X" in
      hello) echo "matched hello";;
      *) echo "no match";;
    esac
  `);
  t.is(r.stdout.trim(), "matched hello");
});

test("case statement wildcard", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    X=unknown
    case "$X" in
      hello) echo "matched hello";;
      *) echo "wildcard";;
    esac
  `);
  t.is(r.stdout.trim(), "wildcard");
});

test("case statement with pattern", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    FILE=image.png
    case "$FILE" in
      *.png) echo "png";;
      *.jpg) echo "jpg";;
      *) echo "other";;
    esac
  `);
  t.is(r.stdout.trim(), "png");
});

// ============================================================================
// Functions
// ============================================================================

test("function definition and call", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    greet() { echo "Hello, $1"; }
    greet World
  `);
  t.is(r.stdout.trim(), "Hello, World");
});

test("function with return value", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    is_even() {
      if [ $(($1 % 2)) -eq 0 ]; then return 0; else return 1; fi
    }
    is_even 4 && echo even || echo odd
  `);
  t.is(r.stdout.trim(), "even");
});

test("function with local variable", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    outer=global
    myfn() {
      local outer=local
      echo $outer
    }
    myfn
    echo $outer
  `);
  t.is(r.stdout.trim(), "local\nglobal");
});

test("recursive function", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    factorial() {
      if [ $1 -le 1 ]; then echo 1; return; fi
      local sub=$(factorial $(($1 - 1)))
      echo $(($1 * sub))
    }
    factorial 5
  `);
  t.is(r.stdout.trim(), "120");
});

test("function persists across calls", (t) => {
  const bash = new Bash();
  bash.executeSync("add() { echo $(($1 + $2)); }");
  t.is(bash.executeSync("add 10 20").stdout.trim(), "30");
});

// ============================================================================
// Exit codes
// ============================================================================

test("exit code from exit command", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("exit 0").exitCode, 0);
  t.is(bash.executeSync("exit 42").exitCode, 42);
});

test("$? captures last exit code", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("false; echo $?");
  t.is(r.stdout.trim(), "1");
});

// ============================================================================
// Subshell
// ============================================================================

test("subshell does not leak variables", (t) => {
  const bash = new Bash();
  bash.executeSync("(X=inner)");
  t.is(bash.executeSync("echo ${X:-unset}").stdout.trim(), "unset");
});

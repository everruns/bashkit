import test from "ava";
import { Bash } from "../wrapper.js";

// ============================================================================
// Text processing — cat, head, tail, wc
// ============================================================================

test("cat reads file", (t) => {
  const bash = new Bash();
  bash.executeSync('echo "hello" > /tmp/cat.txt');
  t.is(bash.executeSync("cat /tmp/cat.txt").stdout.trim(), "hello");
});

test("cat concatenates files", (t) => {
  const bash = new Bash();
  bash.executeSync('echo "a" > /tmp/a.txt');
  bash.executeSync('echo "b" > /tmp/b.txt');
  const r = bash.executeSync("cat /tmp/a.txt /tmp/b.txt");
  t.true(r.stdout.includes("a"));
  t.true(r.stdout.includes("b"));
});

test("head -n limits lines", (t) => {
  const bash = new Bash();
  bash.executeSync('echo -e "1\\n2\\n3\\n4\\n5" > /tmp/h.txt');
  const r = bash.executeSync("head -n 2 /tmp/h.txt");
  t.is(r.stdout.trim(), "1\n2");
});

test("tail -n limits lines", (t) => {
  const bash = new Bash();
  bash.executeSync('echo -e "1\\n2\\n3\\n4\\n5" > /tmp/t.txt');
  const r = bash.executeSync("tail -n 2 /tmp/t.txt");
  t.is(r.stdout.trim(), "4\n5");
});

test("wc -l counts lines", (t) => {
  const bash = new Bash();
  bash.executeSync('echo -e "a\\nb\\nc" > /tmp/wc.txt');
  const r = bash.executeSync("wc -l < /tmp/wc.txt");
  t.is(r.stdout.trim(), "3");
});

test("wc -w counts words", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo 'one two three' | wc -w");
  t.is(r.stdout.trim(), "3");
});

// ============================================================================
// Text processing — grep
// ============================================================================

test("grep basic match", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo -e "apple\\nbanana\\ncherry" | grep banana');
  t.is(r.stdout.trim(), "banana");
});

test("grep -i case insensitive", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo -e "Hello\\nworld" | grep -i hello');
  t.is(r.stdout.trim(), "Hello");
});

test("grep -v inverted match", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo -e "a\\nb\\nc" | grep -v b');
  t.is(r.stdout.trim(), "a\nc");
});

test("grep -c count matches", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo -e "aa\\nab\\nac\\nbb" | grep -c "^a"');
  t.is(r.stdout.trim(), "3");
});

test("grep no match returns non-zero", (t) => {
  const bash = new Bash();
  t.not(bash.executeSync("echo hello | grep xyz").exitCode, 0);
});

// ============================================================================
// Text processing — sed
// ============================================================================

test("sed substitute", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync("echo 'hello world' | sed 's/world/earth/'").stdout.trim(),
    "hello earth"
  );
});

test("sed global substitute", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync("echo 'aaa' | sed 's/a/b/g'").stdout.trim(),
    "bbb"
  );
});

test("sed delete line", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo -e "a\\nb\\nc" | sed "/b/d"');
  t.is(r.stdout.trim(), "a\nc");
});

// ============================================================================
// Text processing — awk
// ============================================================================

test("awk print field", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync("echo 'one two three' | awk '{print $2}'").stdout.trim(),
    "two"
  );
});

test("awk with separator", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync("echo 'a:b:c' | awk -F: '{print $3}'").stdout.trim(),
    "c"
  );
});

test("awk sum column", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(
    "echo -e '1\\n2\\n3\\n4' | awk '{s+=$1} END {print s}'"
  );
  t.is(r.stdout.trim(), "10");
});

// ============================================================================
// Text processing — sort, uniq, tr, cut
// ============================================================================

test("sort ascending", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync('echo -e "c\\na\\nb" | sort').stdout.trim(),
    "a\nb\nc"
  );
});

test("sort -r descending", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync('echo -e "a\\nb\\nc" | sort -r').stdout.trim(),
    "c\nb\na"
  );
});

test("sort -n numeric", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync('echo -e "10\\n2\\n1\\n20" | sort -n').stdout.trim(),
    "1\n2\n10\n20"
  );
});

test("uniq removes adjacent duplicates", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync('echo -e "a\\na\\nb\\nb\\nc" | uniq').stdout.trim(),
    "a\nb\nc"
  );
});

test("sort | uniq -c counts", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo -e "a\\nb\\na\\na" | sort | uniq -c');
  t.true(r.stdout.includes("3"));
  t.true(r.stdout.includes("a"));
});

test("tr transliterate", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync("echo 'hello' | tr 'a-z' 'A-Z'").stdout.trim(),
    "HELLO"
  );
});

test("tr delete characters", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync("echo 'h-e-l-l-o' | tr -d '-'").stdout.trim(),
    "hello"
  );
});

test("cut field extraction", (t) => {
  const bash = new Bash();
  t.is(
    bash.executeSync("echo 'a,b,c' | cut -d, -f2").stdout.trim(),
    "b"
  );
});

// ============================================================================
// printf
// ============================================================================

test("printf basic", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync('printf "Hello %s" "World"').stdout, "Hello World");
});

test("printf with number", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync('printf "%d + %d = %d" 2 3 5').stdout, "2 + 3 = 5");
});

// ============================================================================
// Environment
// ============================================================================

test("export and env", (t) => {
  const bash = new Bash();
  bash.executeSync("export MY_VAR=hello");
  // Verify exported variable is accessible via expansion
  const r = bash.executeSync("echo $MY_VAR");
  t.is(r.stdout.trim(), "hello");
});

test("unset variable", (t) => {
  const bash = new Bash();
  bash.executeSync("X=123");
  bash.executeSync("unset X");
  t.is(bash.executeSync("echo ${X:-gone}").stdout.trim(), "gone");
});

// ============================================================================
// date
// ============================================================================

test("date runs without error", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("date");
  t.is(r.exitCode, 0);
  t.truthy(r.stdout.trim().length > 0);
});

// ============================================================================
// base64
// ============================================================================

test("base64 encode and decode", (t) => {
  const bash = new Bash();
  const encoded = bash.executeSync("echo -n 'hello' | base64").stdout.trim();
  t.is(encoded, "aGVsbG8=");
  t.is(
    bash.executeSync(`echo -n '${encoded}' | base64 -d`).stdout,
    "hello"
  );
});

// ============================================================================
// seq
// ============================================================================

test("seq generates range", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("seq 1 5").stdout.trim(), "1\n2\n3\n4\n5");
});

test("seq with step", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("seq 0 2 6").stdout.trim(), "0\n2\n4\n6");
});

// ============================================================================
// jq (JSON processing)
// ============================================================================

test("jq extract field", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo \'{"name":"alice"}\' | jq -r ".name"');
  t.is(r.stdout.trim(), "alice");
});

test("jq array length", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo '[1,2,3]' | jq 'length'");
  t.is(r.stdout.trim(), "3");
});

test("jq filter array", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(
    "echo '[1,2,3,4,5]' | jq '[.[] | select(. > 3)]'"
  );
  const arr = JSON.parse(r.stdout);
  t.deepEqual(arr, [4, 5]);
});

// ============================================================================
// Checksum
// ============================================================================

test("md5sum produces hash", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo -n 'hello' | md5sum");
  t.is(r.exitCode, 0);
  t.true(r.stdout.includes("5d41402abc4b2a76b9719d911017c592"));
});

test("sha256sum produces hash", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo -n 'hello' | sha256sum");
  t.is(r.exitCode, 0);
  t.true(
    r.stdout.includes(
      "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    )
  );
});

import test from "ava";
import { Bash, BashTool, getVersion, BashError } from "../wrapper.js";

// ============================================================================
// Version
// ============================================================================

test("getVersion returns a semver string", (t) => {
  const v = getVersion();
  t.regex(v, /^\d+\.\d+\.\d+/);
});

// ============================================================================
// Bash — constructor
// ============================================================================

test("Bash: default constructor", (t) => {
  const bash = new Bash();
  t.truthy(bash);
});

test("Bash: constructor with empty options", (t) => {
  const bash = new Bash({});
  const r = bash.executeSync("echo ok");
  t.is(r.exitCode, 0);
});

test("Bash: constructor with all options", (t) => {
  const bash = new Bash({
    username: "u",
    hostname: "h",
    maxCommands: 1000,
    maxLoopIterations: 500,
  });
  const r = bash.executeSync("whoami");
  t.is(r.stdout.trim(), "u");
});

// ============================================================================
// Bash — basic execution
// ============================================================================

test("Bash: echo command", (t) => {
  const bash = new Bash();
  const result = bash.executeSync('echo "hello"');
  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "hello");
});

test("Bash: echo without quotes", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("echo hello world");
  t.is(result.stdout.trim(), "hello world");
});

test("Bash: echo -n suppresses newline", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("echo -n hello");
  t.is(result.stdout, "hello");
});

test("Bash: echo -e interprets escapes", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("echo -e 'a\\tb'");
  t.true(result.stdout.includes("\t"));
});

test("Bash: empty command", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("");
  t.is(result.exitCode, 0);
});

test("Bash: comment-only command", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("# this is a comment");
  t.is(result.exitCode, 0);
});

test("Bash: true returns 0", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("true").exitCode, 0);
});

test("Bash: false returns non-zero", (t) => {
  const bash = new Bash();
  t.not(bash.executeSync("false").exitCode, 0);
});

// ============================================================================
// Bash — arithmetic
// ============================================================================

test("Bash: basic arithmetic", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("echo $((10 * 5 - 3))").stdout.trim(), "47");
});

test("Bash: division", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("echo $((100 / 4))").stdout.trim(), "25");
});

test("Bash: modulo", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("echo $((17 % 5))").stdout.trim(), "2");
});

test("Bash: nested arithmetic", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("echo $(( (3 + 4) * 2 ))").stdout.trim(), "14");
});

// ============================================================================
// Bash — variables and state
// ============================================================================

test("Bash: variable assignment and expansion", (t) => {
  const bash = new Bash();
  bash.executeSync("NAME=world");
  t.is(bash.executeSync('echo "Hello $NAME"').stdout.trim(), "Hello world");
});

test("Bash: state persists between calls", (t) => {
  const bash = new Bash();
  bash.executeSync("X=42");
  t.is(bash.executeSync("echo $X").stdout.trim(), "42");
});

test("Bash: default value expansion", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("echo ${MISSING:-default}").stdout.trim(), "default");
});

test("Bash: assign default value", (t) => {
  const bash = new Bash();
  bash.executeSync(": ${VAR:=hello}");
  t.is(bash.executeSync("echo $VAR").stdout.trim(), "hello");
});

test("Bash: string length", (t) => {
  const bash = new Bash();
  bash.executeSync("S=hello");
  t.is(bash.executeSync("echo ${#S}").stdout.trim(), "5");
});

test("Bash: substring extraction", (t) => {
  const bash = new Bash();
  bash.executeSync("S=hello_world");
  t.is(bash.executeSync("echo ${S:6}").stdout.trim(), "world");
});

test("Bash: variable substitution prefix removal", (t) => {
  const bash = new Bash();
  bash.executeSync("F=path/to/file.txt");
  t.is(bash.executeSync("echo ${F##*/}").stdout.trim(), "file.txt");
});

test("Bash: variable substitution suffix removal", (t) => {
  const bash = new Bash();
  bash.executeSync("F=file.tar.gz");
  t.is(bash.executeSync("echo ${F%%.*}").stdout.trim(), "file");
});

// ============================================================================
// Bash — filesystem
// ============================================================================

test("Bash: write and read file", (t) => {
  const bash = new Bash();
  bash.executeSync('echo "content" > /tmp/test.txt');
  t.is(bash.executeSync("cat /tmp/test.txt").stdout.trim(), "content");
});

test("Bash: append to file", (t) => {
  const bash = new Bash();
  bash.executeSync('echo "line1" > /tmp/append.txt');
  bash.executeSync('echo "line2" >> /tmp/append.txt');
  const r = bash.executeSync("cat /tmp/append.txt");
  t.true(r.stdout.includes("line1"));
  t.true(r.stdout.includes("line2"));
});

test("Bash: mkdir and ls", (t) => {
  const bash = new Bash();
  bash.executeSync("mkdir -p /tmp/testdir/sub");
  bash.executeSync("touch /tmp/testdir/sub/file.txt");
  const r = bash.executeSync("ls /tmp/testdir/sub");
  t.true(r.stdout.includes("file.txt"));
});

test("Bash: cp file", (t) => {
  const bash = new Bash();
  bash.executeSync('echo "data" > /tmp/src.txt');
  bash.executeSync("cp /tmp/src.txt /tmp/dst.txt");
  t.is(bash.executeSync("cat /tmp/dst.txt").stdout.trim(), "data");
});

test("Bash: mv file", (t) => {
  const bash = new Bash();
  bash.executeSync('echo "moveme" > /tmp/before.txt');
  bash.executeSync("mv /tmp/before.txt /tmp/after.txt");
  t.is(bash.executeSync("cat /tmp/after.txt").stdout.trim(), "moveme");
  t.not(bash.executeSync("cat /tmp/before.txt 2>&1").exitCode, 0);
});

test("Bash: rm file", (t) => {
  const bash = new Bash();
  bash.executeSync("touch /tmp/removeme.txt");
  bash.executeSync("rm /tmp/removeme.txt");
  t.not(bash.executeSync("cat /tmp/removeme.txt 2>&1").exitCode, 0);
});

test("Bash: file test -f", (t) => {
  const bash = new Bash();
  bash.executeSync("touch /tmp/exists.txt");
  t.is(bash.executeSync("test -f /tmp/exists.txt && echo yes").stdout.trim(), "yes");
  t.not(bash.executeSync("test -f /tmp/nope.txt").exitCode, 0);
});

test("Bash: directory test -d", (t) => {
  const bash = new Bash();
  bash.executeSync("mkdir -p /tmp/mydir");
  t.is(bash.executeSync("test -d /tmp/mydir && echo yes").stdout.trim(), "yes");
});

test("Bash: pwd", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("pwd");
  t.is(r.exitCode, 0);
  t.truthy(r.stdout.trim().length > 0);
});

test("Bash: cd and pwd", (t) => {
  const bash = new Bash();
  bash.executeSync("mkdir -p /tmp/navtest");
  bash.executeSync("cd /tmp/navtest");
  t.is(bash.executeSync("pwd").stdout.trim(), "/tmp/navtest");
});

// ============================================================================
// Bash — pipes and redirection
// ============================================================================

test("Bash: pipe echo to grep", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo -e "foo\\nbar\\nbaz" | grep bar');
  t.is(r.stdout.trim(), "bar");
});

test("Bash: pipe chain", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo -e "c\\na\\nb" | sort | head -1');
  t.is(r.stdout.trim(), "a");
});

test("Bash: stderr redirect", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo err >&2");
  t.true(r.stderr.includes("err"));
});

test("Bash: redirect to /dev/null", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo hidden > /dev/null");
  t.is(r.stdout, "");
});

test("Bash: command substitution", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo "result: $(echo 42)"');
  t.is(r.stdout.trim(), "result: 42");
});

// ============================================================================
// Bash — options
// ============================================================================

test("Bash: custom username", (t) => {
  const bash = new Bash({ username: "testuser" });
  t.is(bash.executeSync("whoami").stdout.trim(), "testuser");
});

test("Bash: custom hostname", (t) => {
  const bash = new Bash({ hostname: "testhost" });
  t.is(bash.executeSync("hostname").stdout.trim(), "testhost");
});

test("Bash: username and hostname together", (t) => {
  const bash = new Bash({ username: "alice", hostname: "wonderland" });
  t.is(bash.executeSync("whoami").stdout.trim(), "alice");
  t.is(bash.executeSync("hostname").stdout.trim(), "wonderland");
});

// ============================================================================
// Bash — reset
// ============================================================================

test("Bash: reset clears variables", (t) => {
  const bash = new Bash();
  bash.executeSync("X=42");
  bash.reset();
  t.is(bash.executeSync("echo ${X:-unset}").stdout.trim(), "unset");
});

test("Bash: reset clears files", (t) => {
  const bash = new Bash();
  bash.executeSync('echo "data" > /tmp/resetfile.txt');
  bash.reset();
  t.not(bash.executeSync("cat /tmp/resetfile.txt 2>&1").exitCode, 0);
});

test("Bash: reset preserves username config", (t) => {
  const bash = new Bash({ username: "keeper" });
  bash.executeSync("X=gone");
  bash.reset();
  t.is(bash.executeSync("whoami").stdout.trim(), "keeper");
});

test("Bash: cancel stays sticky until clearCancel", (t) => {
  const bash = new Bash();
  bash.cancel();
  const cancelled = bash.executeSync("echo nope");
  t.not(cancelled.exitCode, 0);

  bash.clearCancel();
  const result = bash.executeSync("echo ok");
  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "ok");
});

test("BashTool: cancel stays sticky until clearCancel", (t) => {
  const tool = new BashTool();
  tool.cancel();
  const cancelled = tool.executeSync("echo nope");
  t.not(cancelled.exitCode, 0);

  tool.clearCancel();
  const result = tool.executeSync("echo ok");
  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "ok");
});

test("Bash: async cancel stays sticky until clearCancel", async (t) => {
  const bash = new Bash();
  setTimeout(() => bash.cancel(), 10);

  const cancelled = await bash.execute("for i in $(seq 1 10000); do echo $i; done");
  t.not(cancelled.exitCode, 0);

  const stillCancelled = await bash.execute("echo nope");
  t.not(stillCancelled.exitCode, 0);

  bash.clearCancel();
  const result = await bash.execute("echo ok");
  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "ok");
});

test("BashTool: async cancel stays sticky until clearCancel", async (t) => {
  const tool = new BashTool();
  setTimeout(() => tool.cancel(), 10);

  const cancelled = await tool.execute("for i in $(seq 1 10000); do echo $i; done");
  t.not(cancelled.exitCode, 0);

  const stillCancelled = await tool.execute("echo nope");
  t.not(stillCancelled.exitCode, 0);

  tool.clearCancel();
  const result = await tool.execute("echo ok");
  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "ok");
});

// ============================================================================
// Bash — executeSyncOrThrow
// ============================================================================

test("Bash: executeSyncOrThrow succeeds on exit 0", (t) => {
  const bash = new Bash();
  const result = bash.executeSyncOrThrow("echo ok");
  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "ok");
});

test("Bash: executeSyncOrThrow throws BashError on failure", (t) => {
  const bash = new Bash();
  const err = t.throws(() => bash.executeSyncOrThrow("false"), {
    instanceOf: BashError,
  });
  t.truthy(err);
});

test("Bash: BashError has exitCode", (t) => {
  const bash = new Bash();
  try {
    bash.executeSyncOrThrow("exit 42");
    t.fail("should have thrown");
  } catch (e) {
    t.true(e instanceof BashError);
    t.is((e as BashError).exitCode, 42);
  }
});

test("Bash: BashError display()", (t) => {
  const bash = new Bash();
  try {
    bash.executeSyncOrThrow("exit 1");
    t.fail("should have thrown");
  } catch (e) {
    t.true((e as BashError).display().includes("BashError"));
  }
});

// ============================================================================
// Multiple instances — isolation
// ============================================================================

test("Multiple Bash instances have isolated variables", (t) => {
  const a = new Bash();
  const b = new Bash();
  a.executeSync("X=from_a");
  b.executeSync("X=from_b");
  t.is(a.executeSync("echo $X").stdout.trim(), "from_a");
  t.is(b.executeSync("echo $X").stdout.trim(), "from_b");
});

test("Multiple Bash instances have isolated filesystems", (t) => {
  const a = new Bash();
  const b = new Bash();
  a.executeSync('echo "a" > /tmp/iso.txt');
  t.not(b.executeSync("cat /tmp/iso.txt 2>&1").exitCode, 0);
});

test("Multiple BashTool instances are isolated", (t) => {
  const a = new BashTool();
  const b = new BashTool();
  a.executeSync("VAR=toolA");
  b.executeSync("VAR=toolB");
  t.is(a.executeSync("echo $VAR").stdout.trim(), "toolA");
  t.is(b.executeSync("echo $VAR").stdout.trim(), "toolB");
});

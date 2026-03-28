// Runtime-agnostic test suite using node:test + node:assert.
// Runs under Node, Bun, and Deno to verify NAPI bindings work across runtimes.
//
// Covers: core execution, variables, filesystem, pipes, VFS API, error handling,
// control flow, builtins, BashTool metadata, security/resource limits, isolation.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const native = require("../index.cjs");
const { Bash, BashTool, getVersion } = native;

// ============================================================================
// Version
// ============================================================================

describe("version", () => {
  it("getVersion returns a semver string", () => {
    assert.match(getVersion(), /^\d+\.\d+\.\d+/);
  });
});

// ============================================================================
// Bash — constructor and basic execution
// ============================================================================

describe("Bash basics", () => {
  it("default constructor", () => {
    const bash = new Bash();
    assert.ok(bash);
  });

  it("echo command", () => {
    const bash = new Bash();
    const r = bash.executeSync('echo "hello"');
    assert.equal(r.exitCode, 0);
    assert.equal(r.stdout.trim(), "hello");
  });

  it("empty command", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("").exitCode, 0);
  });

  it("true returns 0, false returns non-zero", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("true").exitCode, 0);
    assert.notEqual(bash.executeSync("false").exitCode, 0);
  });

  it("arithmetic", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("echo $((10 * 5 - 3))").stdout.trim(), "47");
    assert.equal(bash.executeSync("echo $((17 % 5))").stdout.trim(), "2");
  });

  it("constructor with options", () => {
    const bash = new Bash({
      username: "testuser",
      hostname: "testhost",
      maxCommands: 1000,
      maxLoopIterations: 500,
    });
    assert.equal(bash.executeSync("whoami").stdout.trim(), "testuser");
    assert.equal(bash.executeSync("hostname").stdout.trim(), "testhost");
  });
});

// ============================================================================
// Variables and state
// ============================================================================

describe("variables and state", () => {
  it("variable assignment and expansion", () => {
    const bash = new Bash();
    bash.executeSync("NAME=world");
    assert.equal(bash.executeSync('echo "Hello $NAME"').stdout.trim(), "Hello world");
  });

  it("state persists between calls", () => {
    const bash = new Bash();
    bash.executeSync("X=42");
    assert.equal(bash.executeSync("echo $X").stdout.trim(), "42");
  });

  it("default value expansion", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("echo ${MISSING:-default}").stdout.trim(), "default");
  });

  it("string length", () => {
    const bash = new Bash();
    bash.executeSync("S=hello");
    assert.equal(bash.executeSync("echo ${#S}").stdout.trim(), "5");
  });

  it("prefix/suffix removal", () => {
    const bash = new Bash();
    bash.executeSync("F=path/to/file.txt");
    assert.equal(bash.executeSync("echo ${F##*/}").stdout.trim(), "file.txt");
    bash.executeSync("G=file.tar.gz");
    assert.equal(bash.executeSync("echo ${G%%.*}").stdout.trim(), "file");
  });

  it("string replacement", () => {
    const bash = new Bash();
    bash.executeSync("S='hello world hello'");
    assert.equal(bash.executeSync('echo "${S//hello/bye}"').stdout.trim(), "bye world bye");
  });

  it("uppercase/lowercase conversion", () => {
    const bash = new Bash();
    bash.executeSync("S=hello");
    assert.equal(bash.executeSync('echo "${S^^}"').stdout.trim(), "HELLO");
    bash.executeSync("U=HELLO");
    assert.equal(bash.executeSync('echo "${U,,}"').stdout.trim(), "hello");
  });

  it("arrays", () => {
    const bash = new Bash();
    bash.executeSync("ARR=(apple banana cherry)");
    assert.equal(bash.executeSync('echo "${ARR[0]}"').stdout.trim(), "apple");
    assert.equal(bash.executeSync('echo "${#ARR[@]}"').stdout.trim(), "3");
    bash.executeSync("ARR+=(date)");
    assert.equal(bash.executeSync('echo "${#ARR[@]}"').stdout.trim(), "4");
  });
});

// ============================================================================
// Filesystem
// ============================================================================

describe("filesystem", () => {
  it("write, read, append", () => {
    const bash = new Bash();
    bash.executeSync('echo "line1" > /tmp/f.txt');
    bash.executeSync('echo "line2" >> /tmp/f.txt');
    const r = bash.executeSync("cat /tmp/f.txt");
    assert.ok(r.stdout.includes("line1"));
    assert.ok(r.stdout.includes("line2"));
  });

  it("mkdir, touch, ls", () => {
    const bash = new Bash();
    bash.executeSync("mkdir -p /tmp/d/sub");
    bash.executeSync("touch /tmp/d/sub/file.txt");
    assert.ok(bash.executeSync("ls /tmp/d/sub").stdout.includes("file.txt"));
  });

  it("cp and mv", () => {
    const bash = new Bash();
    bash.executeSync('echo "data" > /tmp/src.txt');
    bash.executeSync("cp /tmp/src.txt /tmp/cp.txt");
    assert.equal(bash.executeSync("cat /tmp/cp.txt").stdout.trim(), "data");
    bash.executeSync("mv /tmp/cp.txt /tmp/mv.txt");
    assert.equal(bash.executeSync("cat /tmp/mv.txt").stdout.trim(), "data");
    assert.notEqual(bash.executeSync("cat /tmp/cp.txt 2>&1").exitCode, 0);
  });

  it("rm and test flags", () => {
    const bash = new Bash();
    bash.executeSync("touch /tmp/rm.txt");
    assert.equal(bash.executeSync("test -f /tmp/rm.txt && echo yes").stdout.trim(), "yes");
    bash.executeSync("rm /tmp/rm.txt");
    assert.notEqual(bash.executeSync("test -f /tmp/rm.txt").exitCode, 0);
  });

  it("cd and pwd", () => {
    const bash = new Bash();
    bash.executeSync("mkdir -p /tmp/nav");
    bash.executeSync("cd /tmp/nav");
    assert.equal(bash.executeSync("pwd").stdout.trim(), "/tmp/nav");
  });
});

// ============================================================================
// Pipes and redirection
// ============================================================================

describe("pipes and redirection", () => {
  it("pipe echo to grep", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync('echo -e "foo\\nbar\\nbaz" | grep bar').stdout.trim(),
      "bar",
    );
  });

  it("pipe chain", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync('echo -e "c\\na\\nb" | sort | head -1').stdout.trim(),
      "a",
    );
  });

  it("stderr redirect", () => {
    const bash = new Bash();
    const r = bash.executeSync("echo err >&2");
    assert.ok(r.stderr.includes("err"));
  });

  it("command substitution", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync('echo "result: $(echo 42)"').stdout.trim(),
      "result: 42",
    );
  });

  it("heredoc", () => {
    const bash = new Bash();
    bash.executeSync("NAME=alice");
    const r = bash.executeSync("cat <<EOF\nhello $NAME\nEOF");
    assert.equal(r.stdout.trim(), "hello alice");
  });
});

// ============================================================================
// Control flow
// ============================================================================

describe("control flow", () => {
  it("if/elif/else", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      X=2
      if [ "$X" = "1" ]; then echo one
      elif [ "$X" = "2" ]; then echo two
      else echo other
      fi
    `);
    assert.equal(r.stdout.trim(), "two");
  });

  it("for loop", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync("for i in a b c; do echo $i; done").stdout.trim(),
      "a\nb\nc",
    );
  });

  it("while loop", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      i=0
      while [ $i -lt 3 ]; do echo $i; i=$((i + 1)); done
    `);
    assert.equal(r.stdout.trim(), "0\n1\n2");
  });

  it("break and continue", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync(`
        for i in 1 2 3 4 5; do
          if [ $i -eq 4 ]; then break; fi
          if [ $i -eq 2 ]; then continue; fi
          echo $i
        done
      `).stdout.trim(),
      "1\n3",
    );
  });

  it("case statement", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      FILE=image.png
      case "$FILE" in
        *.png) echo "png";;
        *.jpg) echo "jpg";;
        *) echo "other";;
      esac
    `);
    assert.equal(r.stdout.trim(), "png");
  });

  it("functions with local vars and recursion", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      factorial() {
        if [ $1 -le 1 ]; then echo 1; return; fi
        local sub=$(factorial $(($1 - 1)))
        echo $(($1 * sub))
      }
      factorial 5
    `);
    assert.equal(r.stdout.trim(), "120");
  });

  it("subshell does not leak variables", () => {
    const bash = new Bash();
    bash.executeSync("(X=inner)");
    assert.equal(bash.executeSync("echo ${X:-unset}").stdout.trim(), "unset");
  });

  it("$? captures last exit code", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("false; echo $?").stdout.trim(), "1");
  });
});

// ============================================================================
// Builtins
// ============================================================================

describe("builtins", () => {
  it("grep variations", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync('echo -e "apple\\nbanana\\ncherry" | grep banana').stdout.trim(),
      "banana",
    );
    assert.equal(
      bash.executeSync('echo -e "Hello\\nworld" | grep -i hello').stdout.trim(),
      "Hello",
    );
    assert.equal(
      bash.executeSync('echo -e "a\\nb\\nc" | grep -v b').stdout.trim(),
      "a\nc",
    );
  });

  it("sed substitute", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync("echo 'aaa' | sed 's/a/b/g'").stdout.trim(),
      "bbb",
    );
  });

  it("awk field extraction and sum", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync("echo 'one two three' | awk '{print $2}'").stdout.trim(),
      "two",
    );
    assert.equal(
      bash.executeSync("echo -e '1\\n2\\n3\\n4' | awk '{s+=$1} END {print s}'").stdout.trim(),
      "10",
    );
  });

  it("sort, uniq, tr, cut", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync('echo -e "c\\na\\nb" | sort').stdout.trim(),
      "a\nb\nc",
    );
    assert.equal(
      bash.executeSync('echo -e "a\\na\\nb\\nb\\nc" | uniq').stdout.trim(),
      "a\nb\nc",
    );
    assert.equal(
      bash.executeSync("echo 'hello' | tr 'a-z' 'A-Z'").stdout.trim(),
      "HELLO",
    );
    assert.equal(
      bash.executeSync("echo 'a,b,c' | cut -d, -f2").stdout.trim(),
      "b",
    );
  });

  it("head, tail, wc", () => {
    const bash = new Bash();
    bash.executeSync('echo -e "1\\n2\\n3\\n4\\n5" > /tmp/hw.txt');
    assert.equal(bash.executeSync("head -n 2 /tmp/hw.txt").stdout.trim(), "1\n2");
    assert.equal(bash.executeSync("tail -n 2 /tmp/hw.txt").stdout.trim(), "4\n5");
    assert.equal(bash.executeSync("wc -l < /tmp/hw.txt").stdout.trim(), "5");
  });

  it("base64 encode/decode", () => {
    const bash = new Bash();
    const encoded = bash.executeSync("echo -n 'hello' | base64").stdout.trim();
    assert.equal(encoded, "aGVsbG8=");
    assert.equal(bash.executeSync(`echo -n '${encoded}' | base64 -d`).stdout, "hello");
  });

  it("jq JSON processing", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync('echo \'{"name":"alice"}\' | jq -r ".name"').stdout.trim(),
      "alice",
    );
    assert.equal(
      bash.executeSync("echo '[1,2,3]' | jq 'length'").stdout.trim(),
      "3",
    );
    const arr = JSON.parse(
      bash.executeSync("echo '[1,2,3,4,5]' | jq '[.[] | select(. > 3)]'").stdout,
    );
    assert.deepEqual(arr, [4, 5]);
  });

  it("md5sum and sha256sum", () => {
    const bash = new Bash();
    assert.ok(
      bash.executeSync("echo -n 'hello' | md5sum").stdout.includes("5d41402abc4b2a76b9719d911017c592"),
    );
    assert.ok(
      bash.executeSync("echo -n 'hello' | sha256sum").stdout.includes(
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
      ),
    );
  });

  it("seq, printf, date, export/unset", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("seq 1 5").stdout.trim(), "1\n2\n3\n4\n5");
    assert.equal(bash.executeSync('printf "Hello %s" "World"').stdout, "Hello World");
    assert.equal(bash.executeSync("date").exitCode, 0);
    bash.executeSync("export MY_VAR=hello");
    assert.equal(bash.executeSync("echo $MY_VAR").stdout.trim(), "hello");
    bash.executeSync("unset MY_VAR");
    assert.equal(bash.executeSync("echo ${MY_VAR:-gone}").stdout.trim(), "gone");
  });
});

// ============================================================================
// VFS API
// ============================================================================

describe("VFS API", () => {
  it("writeFile + readFile roundtrip", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/hello.txt", "Hello, VFS!");
    assert.equal(bash.readFile("/tmp/hello.txt"), "Hello, VFS!");
  });

  it("writeFile overwrites", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/o.txt", "first");
    bash.writeFile("/tmp/o.txt", "second");
    assert.equal(bash.readFile("/tmp/o.txt"), "second");
  });

  it("readFile throws on missing file", () => {
    const bash = new Bash();
    assert.throws(() => bash.readFile("/nonexistent/file.txt"));
  });

  it("mkdir, exists, remove", () => {
    const bash = new Bash();
    bash.mkdir("/tmp/vdir");
    assert.ok(bash.exists("/tmp/vdir"));
    bash.writeFile("/tmp/vdir/f.txt", "data");
    assert.ok(bash.exists("/tmp/vdir/f.txt"));
    bash.remove("/tmp/vdir", true);
    assert.ok(!bash.exists("/tmp/vdir"));
  });

  it("mkdir recursive", () => {
    const bash = new Bash();
    bash.mkdir("/a/b/c/d", true);
    assert.ok(bash.exists("/a/b/c/d"));
    assert.ok(bash.exists("/a/b"));
  });

  it("VFS ↔ bash interop", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/from-vfs.txt", "vfs-content");
    assert.equal(bash.executeSync("cat /tmp/from-vfs.txt").stdout, "vfs-content");
    bash.executeSync("echo bash-content > /tmp/from-bash.txt");
    assert.equal(bash.readFile("/tmp/from-bash.txt"), "bash-content\n");
  });

  it("reset clears VFS state", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/p.txt", "data");
    assert.ok(bash.exists("/tmp/p.txt"));
    bash.reset();
    assert.ok(!bash.exists("/tmp/p.txt"));
  });
});

// ============================================================================
// Error handling
// ============================================================================

describe("error handling", () => {
  it("failed command has non-zero exit code", () => {
    const bash = new Bash();
    assert.notEqual(bash.executeSync("false").exitCode, 0);
  });

  it("exit with specific codes", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("exit 0").exitCode, 0);
    assert.equal(bash.executeSync("exit 1").exitCode, 1);
    assert.equal(bash.executeSync("exit 127").exitCode, 127);
  });

  it("stderr captured separately", () => {
    const bash = new Bash();
    const r = bash.executeSync("echo out; echo err >&2");
    assert.ok(r.stdout.includes("out"));
    assert.ok(r.stderr.includes("err"));
  });

  it("executeSyncOrThrow succeeds on exit 0", () => {
    const bash = new Bash();
    const r = bash.executeSyncOrThrow("echo ok");
    assert.equal(r.exitCode, 0);
    assert.equal(r.stdout.trim(), "ok");
  });

  it("executeSyncOrThrow throws on failure", () => {
    const bash = new Bash();
    assert.throws(() => bash.executeSyncOrThrow("exit 42"), (err) => {
      assert.equal(err.name, "BashError");
      assert.equal(err.exitCode, 42);
      assert.equal(typeof err.message, "string");
      assert.ok(err.display().includes("BashError"));
      return true;
    });
  });

  it("interpreter usable after error, state preserved", () => {
    const bash = new Bash();
    bash.executeSync("X=before");
    bash.executeSync("false");
    assert.equal(bash.executeSync("echo $X").stdout.trim(), "before");
    assert.equal(bash.executeSync("echo recovered").stdout.trim(), "recovered");
  });

  it("syntax error returns non-zero", () => {
    const bash = new Bash();
    assert.notEqual(bash.executeSync("if then fi").exitCode, 0);
  });

  it("pre-exec parse error surfaces in stderr", () => {
    const bash = new Bash();
    const r = bash.executeSync("echo $(");
    assert.notEqual(r.exitCode, 0);
    assert.ok(r.error);
    assert.ok(r.stderr.length > 0);
  });
});

// ============================================================================
// Reset
// ============================================================================

describe("reset", () => {
  it("clears variables and files", () => {
    const bash = new Bash();
    bash.executeSync("X=42");
    bash.executeSync('echo "data" > /tmp/r.txt');
    bash.reset();
    assert.equal(bash.executeSync("echo ${X:-unset}").stdout.trim(), "unset");
    assert.notEqual(bash.executeSync("cat /tmp/r.txt 2>&1").exitCode, 0);
  });

  it("preserves config after reset", () => {
    const bash = new Bash({ username: "keeper" });
    bash.executeSync("X=gone");
    bash.reset();
    assert.equal(bash.executeSync("whoami").stdout.trim(), "keeper");
  });
});

// ============================================================================
// BashTool metadata
// ============================================================================

describe("BashTool metadata", () => {
  it("name, version, shortDescription", () => {
    const tool = new BashTool();
    assert.equal(tool.name, "bashkit");
    assert.match(tool.version, /^\d+\.\d+\.\d+/);
    assert.equal(tool.version, getVersion());
    assert.ok(tool.shortDescription.length > 0);
  });

  it("description, help, systemPrompt", () => {
    const tool = new BashTool();
    assert.ok(tool.description().length > 10);
    assert.ok(tool.help().length > 10);
    assert.notEqual(tool.description(), tool.help());
    assert.ok(tool.systemPrompt().toLowerCase().includes("bash"));
  });

  it("inputSchema and outputSchema are valid JSON", () => {
    const tool = new BashTool();
    const input = JSON.parse(tool.inputSchema());
    const output = JSON.parse(tool.outputSchema());
    assert.equal(typeof input, "object");
    assert.equal(typeof output, "object");
    assert.ok(JSON.stringify(input).includes("command"));
  });

  it("schemas stable across calls and instances", () => {
    const a = new BashTool();
    const b = new BashTool();
    assert.equal(a.inputSchema(), a.inputSchema());
    assert.equal(a.inputSchema(), b.inputSchema());
    assert.equal(a.outputSchema(), b.outputSchema());
  });

  it("metadata unchanged after execution and reset", () => {
    const tool = new BashTool();
    const nameBefore = tool.name;
    const schemaBefore = tool.inputSchema();
    tool.executeSync("echo hello");
    tool.reset();
    assert.equal(tool.name, nameBefore);
    assert.equal(tool.inputSchema(), schemaBefore);
  });

  it("systemPrompt reflects configured username", () => {
    const tool = new BashTool({ username: "agent", hostname: "sandbox" });
    const prompt = tool.systemPrompt();
    assert.ok(prompt.includes("agent"));
    assert.ok(prompt.includes("/home/agent"));
  });

  it("BashTool execution and reset", () => {
    const tool = new BashTool({ username: "keep" });
    tool.executeSync("VAR=gone");
    tool.reset();
    assert.equal(tool.executeSync("echo ${VAR:-unset}").stdout.trim(), "unset");
    assert.equal(tool.executeSync("whoami").stdout.trim(), "keep");
  });
});

// ============================================================================
// Security / resource limits
// ============================================================================

describe("security", () => {
  it("command limit enforced", () => {
    const bash = new Bash({ maxCommands: 5 });
    const r = bash.executeSync("true; true; true; true; true; true; true; true; true; true");
    assert.ok(r.exitCode !== 0 || r.error !== undefined);
  });

  it("loop iteration limit enforced", () => {
    const bash = new Bash({ maxLoopIterations: 5 });
    const r = bash.executeSync("for i in 1 2 3 4 5 6 7 8 9 10; do echo $i; done");
    assert.ok(r.exitCode !== 0 || r.error !== undefined);
  });

  it("infinite while loop capped", () => {
    const bash = new Bash({ maxLoopIterations: 10 });
    const r = bash.executeSync("i=0; while true; do i=$((i+1)); done");
    assert.ok(r.exitCode !== 0 || r.error !== undefined);
  });

  it("recursive function depth limited", () => {
    const bash = new Bash({ maxCommands: 10000 });
    const r = bash.executeSync("bomb() { bomb; }; bomb");
    assert.ok(r.exitCode !== 0 || r.error !== undefined);
  });

  it("sandbox escape blocked", () => {
    const bash = new Bash();
    assert.notEqual(bash.executeSync("exec /bin/bash").exitCode, 0);
    assert.notEqual(bash.executeSync("cat /proc/self/maps 2>&1").exitCode, 0);
    assert.notEqual(bash.executeSync("cat /etc/passwd 2>&1").exitCode, 0);
  });

  it("VFS path traversal blocked", () => {
    const bash = new Bash();
    bash.executeSync('echo "secret" > /home/data.txt');
    assert.notEqual(bash.executeSync("cat /home/../../../etc/shadow 2>&1").exitCode, 0);
  });

  it("recovery after exceeding limits", () => {
    const bash = new Bash({ maxCommands: 3 });
    bash.executeSync("true; true; true; true; true; true");
    const r = bash.executeSync("echo recovered");
    assert.equal(r.exitCode, 0);
    assert.equal(r.stdout.trim(), "recovered");
  });
});

// ============================================================================
// Instance isolation
// ============================================================================

describe("isolation", () => {
  it("Bash instances have isolated variables", () => {
    const a = new Bash();
    const b = new Bash();
    a.executeSync("X=from_a");
    b.executeSync("X=from_b");
    assert.equal(a.executeSync("echo $X").stdout.trim(), "from_a");
    assert.equal(b.executeSync("echo $X").stdout.trim(), "from_b");
  });

  it("Bash instances have isolated filesystems", () => {
    const a = new Bash();
    const b = new Bash();
    a.executeSync('echo "a" > /tmp/iso.txt');
    assert.notEqual(b.executeSync("cat /tmp/iso.txt 2>&1").exitCode, 0);
  });

  it("BashTool instances are isolated", () => {
    const a = new BashTool();
    const b = new BashTool();
    a.executeSync("VAR=toolA");
    b.executeSync("VAR=toolB");
    assert.equal(a.executeSync("echo $VAR").stdout.trim(), "toolA");
    assert.equal(b.executeSync("echo $VAR").stdout.trim(), "toolB");
  });
});

// ============================================================================
// Real-world script patterns
// ============================================================================

describe("scripts", () => {
  it("JSON processing pipeline", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      echo '[{"name":"alice","age":30},{"name":"bob","age":25}]' | \
        jq -r '.[] | select(.age > 28) | .name'
    `);
    assert.equal(r.stdout.trim(), "alice");
  });

  it("data transformation pipeline", () => {
    const bash = new Bash();
    bash.executeSync('echo -e "Alice,30\\nBob,25\\nCharlie,35" > /tmp/data.csv');
    assert.equal(
      bash.executeSync("cat /tmp/data.csv | sort -t, -k2 -n | head -1 | cut -d, -f1").stdout.trim(),
      "Bob",
    );
  });

  it("config file generation via heredoc", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      APP_NAME=myapp
      APP_PORT=8080
      cat <<EOF
{
  "name": "$APP_NAME",
  "port": $APP_PORT
}
EOF
    `);
    const config = JSON.parse(r.stdout);
    assert.equal(config.name, "myapp");
    assert.equal(config.port, 8080);
  });

  it("many sequential commands", () => {
    const bash = new Bash();
    for (let i = 0; i < 50; i++) {
      bash.executeSync(`echo ${i}`);
    }
    assert.equal(bash.executeSync("echo done").stdout.trim(), "done");
  });

  it("large output", () => {
    const bash = new Bash();
    const r = bash.executeSync("seq 1 1000");
    const lines = r.stdout.trim().split("\n");
    assert.equal(lines.length, 1000);
    assert.equal(lines[0], "1");
    assert.equal(lines[999], "1000");
  });
});

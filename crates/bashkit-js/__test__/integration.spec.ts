import test from "ava";
import { Bash, BashError, BashTool, ScriptedTool } from "../wrapper.js";

function makeCrudTool(): ScriptedTool {
  const db = new Map<string, string>();
  const tool = new ScriptedTool({
    name: "crud_api",
    shortDescription: "CRUD API",
  });
  const schema = {
    type: "object",
    properties: {
      key: { type: "string" },
      value: { type: "string" },
    },
  };

  tool.addTool(
    "create",
    "Create a record",
    (params) => {
      const key = String(params.key ?? "");
      const value = String(params.value ?? "");
      db.set(key, value);
      return `${JSON.stringify({ created: key })}\n`;
    },
    schema,
  );

  tool.addTool(
    "read",
    "Read a record",
    (params) => {
      const key = String(params.key ?? "");
      if (db.has(key)) {
        return `${JSON.stringify({ key, value: db.get(key) })}\n`;
      }
      return `${JSON.stringify({ error: "not found" })}\n`;
    },
    schema,
  );

  tool.addTool("list_all", "List all keys", () => {
    return `${JSON.stringify([...db.keys()])}\n`;
  });

  tool.addTool(
    "delete",
    "Delete a record",
    (params) => {
      const key = String(params.key ?? "");
      if (db.has(key)) {
        db.delete(key);
        return `${JSON.stringify({ deleted: key })}\n`;
      }
      return `${JSON.stringify({ error: "not found" })}\n`;
    },
    schema,
  );

  return tool;
}

// ============================================================================
// Multi-step workflows
// ============================================================================

test("integration: multi-step file workflow", (t) => {
  const bash = new Bash();
  bash.executeSync("echo 'initial content' > /tmp/workflow.txt");
  bash.executeSync("echo 'appended line' >> /tmp/workflow.txt");

  const lineCount = bash.executeSync("wc -l < /tmp/workflow.txt");
  t.is(lineCount.exitCode, 0);
  t.is(lineCount.stdout.trim(), "2");

  const head = bash.executeSync("head -1 /tmp/workflow.txt");
  t.is(head.stdout.trim(), "initial content");
});

test("integration: directory tree workflow", (t) => {
  const bash = new Bash();
  bash.executeSync("mkdir -p /tmp/project/src /tmp/project/tests");
  bash.executeSync("echo 'fn main() {}' > /tmp/project/src/main.rs");
  bash.executeSync("echo '[test]' > /tmp/project/tests/test.rs");

  const result = bash.executeSync("find /tmp/project -type f | sort");
  t.is(result.exitCode, 0);
  t.true(result.stdout.includes("/tmp/project/src/main.rs"));
  t.true(result.stdout.includes("/tmp/project/tests/test.rs"));
});

test("integration: pipeline data processing", (t) => {
  const bash = new Bash();
  const result = bash.executeSync(`
    printf 'apple\nbanana\napple\ncherry\nbanana\n' \
      | sort \
      | uniq \
      | grep -E '^(apple|cherry)$'
  `);

  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "apple\ncherry");
});

test("integration: variable computation workflow", (t) => {
  const bash = new Bash();
  bash.executeSync("SUM=0");
  bash.executeSync("for i in 1 2 3 4 5; do SUM=$((SUM + i)); done");

  const result = bash.executeSync("echo $SUM");
  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "15");
});

// ============================================================================
// Sync/async interleaving
// ============================================================================

test("integration: async then sync on same instance", async (t) => {
  const bash = new Bash();
  const asyncResult = await bash.execute("export PHASE=async");
  t.is(asyncResult.exitCode, 0);

  const syncResult = bash.executeSync("echo $PHASE");
  t.is(syncResult.exitCode, 0);
  t.is(syncResult.stdout.trim(), "async");
});

test("integration: sync then async on same instance", async (t) => {
  const bash = new Bash();
  const syncResult = bash.executeSync("export MODE=sync");
  t.is(syncResult.exitCode, 0);

  const asyncResult = await bash.execute("echo $MODE");
  t.is(asyncResult.exitCode, 0);
  t.is(asyncResult.stdout.trim(), "sync");
});

test("integration: interleaved file operations", async (t) => {
  const bash = new Bash();
  bash.executeSync("echo line1 > /tmp/interleave.txt");
  await bash.execute("echo line2 >> /tmp/interleave.txt");
  bash.executeSync("echo line3 >> /tmp/interleave.txt");

  const result = await bash.execute("cat /tmp/interleave.txt");
  t.is(result.exitCode, 0);
  t.deepEqual(result.stdout.trim().split("\n"), ["line1", "line2", "line3"]);
});

// ============================================================================
// CRUD patterns
// ============================================================================

test("integration: CRUD workflow", (t) => {
  const bash = new Bash();
  bash.executeSync("echo alpha > /tmp/item.txt");
  t.is(bash.executeSync("cat /tmp/item.txt").stdout.trim(), "alpha");

  bash.executeSync("echo beta > /tmp/item.txt");
  t.is(bash.executeSync("cat /tmp/item.txt").stdout.trim(), "beta");

  bash.executeSync("rm /tmp/item.txt");
  t.not(bash.executeSync("cat /tmp/item.txt 2>&1").exitCode, 0);
});

test("integration: CRUD with conditional logic", (t) => {
  const bash = new Bash();
  const result = bash.executeSync(`
    FILE=/tmp/config.txt
    if [ -f "$FILE" ]; then
      echo "updated" > "$FILE"
    else
      echo "created" > "$FILE"
    fi
    cat "$FILE"
  `);

  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "created");
});

test("integration: CRUD error handling", (t) => {
  const bash = new Bash();
  const result = bash.executeSync(`
    if cat /tmp/missing.txt >/dev/null 2>&1; then
      echo "unexpected"
    else
      echo "created" > /tmp/missing.txt
    fi
    cat /tmp/missing.txt
  `);

  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "created");
});

// ============================================================================
// ScriptedTool workflows
// ============================================================================

test("integration: chained pipes through scripted tools", async (t) => {
  const tool = new ScriptedTool({ name: "transform" });
  tool.addTool("upper", "Uppercase stdin", (_params, stdin) =>
    (stdin ?? "").toUpperCase(),
  );
  tool.addTool(
    "prefix",
    "Add prefix",
    (_params, stdin) => `PREFIX:${stdin ?? ""}`,
  );

  const result = await tool.execute("echo hello | upper | prefix");
  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "PREFIX:HELLO");
});

test("integration: loop with accumulation in scripted tool", async (t) => {
  const tool = new ScriptedTool({ name: "calc" });
  tool.addTool(
    "double",
    "Double a number",
    (params) => `${Number(params.n ?? 0) * 2}\n`,
    { type: "object", properties: { n: { type: "integer" } } },
  );

  const result = await tool.execute(`
    result=""
    for i in 1 2 3 4 5; do
      val=$(double --n $i)
      result="$result $val"
    done
    echo $result
  `);

  t.is(result.exitCode, 0);
  t.deepEqual(result.stdout.trim().split(/\s+/), ["2", "4", "6", "8", "10"]);
});

test("integration: scripted tool keeps callback references alive", (t) => {
  const tool = new ScriptedTool({ name: "gc_guard" });
  tool.addTool("one", "One", () => "1\n");
  tool.addTool("two", "Two", () => "2\n");

  t.is((tool as unknown as { callbackRefs: unknown[] }).callbackRefs.length, 2);
});

// ============================================================================
// Reset behavior
// ============================================================================

test("integration: reset clears files and vars", (t) => {
  const bash = new Bash();
  bash.executeSync("export MYVAR=hello");
  bash.executeSync("echo data > /tmp/resettest.txt");
  bash.reset();

  t.is(bash.executeSync("echo ${MYVAR:-cleared}").stdout.trim(), "cleared");
  t.not(bash.executeSync("cat /tmp/resettest.txt").exitCode, 0);
});

test("integration: BashTool reset clears state", (t) => {
  const tool = new BashTool({ username: "testuser" });
  tool.executeSync("export SECRET=123");
  tool.executeSync("echo data > /tmp/toolreset.txt");
  tool.reset();

  t.is(tool.executeSync("echo ${SECRET:-cleared}").stdout.trim(), "cleared");
  t.is(tool.executeSync("whoami").stdout.trim(), "testuser");
});

test("integration: BashTool snapshot roundtrip preserves state and config", (t) => {
  const tool = new BashTool({
    username: "agent",
    maxCommands: 5,
    maxLoopIterations: 50,
  });
  tool.executeSync(
    "export BUILD_ID=42; mkdir -p /workspace && cd /workspace && echo ready > state.txt",
  );

  const snapshot = tool.snapshot();
  const restored = BashTool.fromSnapshot(snapshot, {
    username: "agent",
    maxCommands: 5,
    maxLoopIterations: 50,
  });

  t.is(restored.executeSync("echo $BUILD_ID").stdout.trim(), "42");
  t.is(restored.executeSync("cat /workspace/state.txt").stdout.trim(), "ready");
  t.is(restored.executeSync("pwd").stdout.trim(), "/workspace");
  t.is(restored.executeSync("whoami").stdout.trim(), "agent");

  const limited = restored.executeSync("true; true; true; true; true; true");
  t.true(
    limited.exitCode !== 0 || limited.error !== undefined,
    "fromSnapshot() must preserve maxCommands",
  );
});

test("integration: BashTool restoreSnapshot after reset restores original state", (t) => {
  const tool = new BashTool({ username: "agent" });
  tool.executeSync("export SNAP=yes; mkdir -p /tmp/restore && cd /tmp/restore");

  const snapshot = tool.snapshot();

  tool.reset();
  t.is(tool.executeSync("echo ${SNAP:-missing}").stdout.trim(), "missing");

  tool.restoreSnapshot(snapshot);
  t.is(tool.executeSync("echo $SNAP").stdout.trim(), "yes");
  t.is(tool.executeSync("pwd").stdout.trim(), "/tmp/restore");
  t.is(tool.executeSync("whoami").stdout.trim(), "agent");
});

test("integration: Bash snapshot can exclude filesystem", (t) => {
  const bash = new Bash();
  bash.executeSync("export KEEP=1; echo saved > /tmp/state.txt");

  const snapshot = bash.snapshot({ excludeFilesystem: true });

  bash.executeSync("export KEEP=2; echo changed > /tmp/state.txt");
  bash.restoreSnapshot(snapshot);

  t.is(bash.executeSync("echo $KEEP").stdout.trim(), "1");
  t.is(bash.executeSync("cat /tmp/state.txt").stdout.trim(), "changed");
});

test("integration: BashTool snapshot can exclude filesystem", (t) => {
  const tool = new BashTool();
  tool.executeSync("export KEEP=1; echo saved > /tmp/tool.txt");

  const snapshot = tool.snapshot({ excludeFilesystem: true });

  tool.executeSync("export KEEP=2; echo changed > /tmp/tool.txt");
  tool.restoreSnapshot(snapshot);

  t.is(tool.executeSync("echo $KEEP").stdout.trim(), "1");
  t.is(tool.executeSync("cat /tmp/tool.txt").stdout.trim(), "changed");
});

test("integration: BashTool empty snapshot roundtrip works", (t) => {
  const tool = new BashTool();
  const expectedPwd = tool.executeSync("pwd").stdout.trim();
  const snapshot = tool.snapshot();
  const restored = BashTool.fromSnapshot(snapshot);

  t.is(restored.executeSync("pwd").stdout.trim(), expectedPwd);
  t.is(restored.executeSync("echo ${MISSING:-unset}").stdout.trim(), "unset");
});

test("integration: BashTool invalid snapshot throws", (t) => {
  const tool = new BashTool();
  const invalid = new Uint8Array([0, 1, 2, 3, 4]);

  t.throws(() => tool.restoreSnapshot(invalid));
  t.throws(() => BashTool.fromSnapshot(invalid));
});

test("integration: multiple resets remain stable", (t) => {
  const bash = new Bash();
  for (let i = 0; i < 10; i++) {
    bash.executeSync(`export V${i}=val${i}`);
    bash.reset();
  }

  const result = bash.executeSync("echo ok");
  t.is(result.exitCode, 0);
  t.is(result.stdout.trim(), "ok");
});

// ============================================================================
// Concurrency
// ============================================================================

test("integration: concurrent Bash instances keep isolated state", async (t) => {
  const instances = Array.from({ length: 4 }, () => new Bash());
  const results = await Promise.all(
    instances.map((bash, index) =>
      bash.execute(
        `echo thread-${index}; echo value-${index} > /tmp/file.txt; cat /tmp/file.txt`,
      ),
    ),
  );

  for (const [index, result] of results.entries()) {
    t.is(result.exitCode, 0);
    t.true(result.stdout.includes(`thread-${index}`));
    t.true(result.stdout.includes(`value-${index}`));
  }
});

test("integration: concurrent ScriptedTool instances stay isolated", async (t) => {
  const tools = Array.from({ length: 3 }, (_, index) => {
    const tool = new ScriptedTool({ name: `api_${index}` });
    tool.addTool("id", "Return ID", () => `tool-${index}\n`);
    return tool;
  });

  const results = await Promise.all(tools.map((tool) => tool.execute("id")));
  for (const [index, result] of results.entries()) {
    t.is(result.exitCode, 0);
    t.is(result.stdout.trim(), `tool-${index}`);
  }
});

test("integration: async concurrent BashTool executions on different instances", async (t) => {
  const tools = Array.from(
    { length: 3 },
    (_, index) => new BashTool({ username: `agent${index}` }),
  );

  const results = await Promise.all(
    tools.map((tool, index) => tool.execute(`echo ${index}; whoami`)),
  );

  for (const [index, result] of results.entries()) {
    t.is(result.exitCode, 0);
    t.true(result.stdout.includes(String(index)));
    t.true(result.stdout.includes(`agent${index}`));
  }
});

test("integration: async ScriptedTool concurrent execution", async (t) => {
  const tool = new ScriptedTool({ name: "multi" });
  tool.addTool(
    "prefix",
    "Prefix stdin",
    (params, stdin) => `${String(params.tag ?? "tag")}:${stdin ?? ""}`,
    { type: "object", properties: { tag: { type: "string" } } },
  );

  const [first, second] = await Promise.all([
    tool.execute("echo one | prefix --tag first"),
    tool.execute("echo two | prefix --tag second"),
  ]);

  t.is(first.exitCode, 0);
  t.is(second.exitCode, 0);
  t.is(first.stdout.trim(), "first:one");
  t.is(second.stdout.trim(), "second:two");
});

// ============================================================================
// ExecResult contract
// ============================================================================

test("integration: ExecResult shape contract", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("echo hello");
  for (const key of [
    "stdout",
    "stderr",
    "exitCode",
    "stdoutTruncated",
    "stderrTruncated",
    "success",
  ]) {
    t.true(key in result);
  }
  t.is(result.error, undefined);
});

test("integration: success matches exitCode === 0", (t) => {
  const bash = new Bash();
  const ok = bash.executeSync("true");
  const fail = bash.executeSync("false");

  t.true(ok.success);
  t.is(ok.exitCode, 0);
  t.false(fail.success);
  t.not(fail.exitCode, 0);
});

test("integration: parse failure sets error field", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("echo $(");

  t.not(result.exitCode, 0);
  t.truthy(result.error);
  t.truthy(result.stderr);
});

// ============================================================================
// Tool metadata integration
// ============================================================================

test("integration: BashTool schemas are valid JSON", (t) => {
  const tool = new BashTool();
  t.truthy(JSON.parse(tool.inputSchema()));
  t.truthy(JSON.parse(tool.outputSchema()));
});

test("integration: BashTool system prompt includes configured username", (t) => {
  const tool = new BashTool({ username: "agent007" });
  const prompt = tool.systemPrompt();
  t.true(prompt.includes("agent007"));
  t.true(prompt.includes("/home/agent007"));
});

test("integration: BashTool version format is semver", (t) => {
  const tool = new BashTool();
  t.regex(tool.version, /^\d+\.\d+\.\d+/);
});

test("integration: ScriptedTool system prompt lists all registered tools", (t) => {
  const tool = new ScriptedTool({ name: "multi" });
  for (const name of ["alpha", "beta", "gamma"]) {
    tool.addTool(name, `Tool ${name}`, () => "ok\n");
  }

  const prompt = tool.systemPrompt();
  t.true(prompt.includes("alpha"));
  t.true(prompt.includes("beta"));
  t.true(prompt.includes("gamma"));
});

test("integration: ScriptedTool help is valid markdown", (t) => {
  const tool = new ScriptedTool({ name: "myapi" });
  tool.addTool("cmd", "A command", () => "ok\n");

  const help = tool.help();
  t.true(help.includes("# myapi"));
  t.true(help.includes("cmd"));
});

// ============================================================================
// Stress / edge cases
// ============================================================================

test("integration: very long command", (t) => {
  const bash = new Bash();
  const payload = "x".repeat(10_000);
  const result = bash.executeSync(`echo '${payload}'`);

  t.is(result.exitCode, 0);
  t.is(result.stdout.trim().length, 10_000);
});

test("integration: many sequential commands", (t) => {
  const bash = new Bash();
  const commands = Array.from(
    { length: 120 },
    (_, index) => `echo line${index}`,
  ).join("; ");
  const result = bash.executeSync(commands);

  t.is(result.exitCode, 0);
  t.is(result.stdout.trim().split("\n").length, 120);
});

test("integration: empty pipeline stages", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("echo '' | cat | cat");

  t.is(result.exitCode, 0);
});

test("integration: async error propagation", async (t) => {
  const bash = new Bash();
  const result = await bash.execute("exit 42");
  t.is(result.exitCode, 42);
  t.false(result.success);

  await t.throwsAsync(() => bash.executeOrThrow("exit 42"), {
    instanceOf: BashError,
  });
});

test("integration: ScriptedTool CRUD workflow", async (t) => {
  const tool = makeCrudTool();
  const result = await tool.execute(`
    create --key user1 --value Alice
    create --key user2 --value Bob
    list_all | jq -r '.[]' | sort
  `);

  t.is(result.exitCode, 0);
  t.true(result.stdout.includes("user1"));
  t.true(result.stdout.includes("user2"));
});

test("integration: ScriptedTool CRUD with conditional logic", async (t) => {
  const tool = makeCrudTool();
  const result = await tool.execute(`
    create --key config --value enabled
    status=$(read --key config | jq -r '.value')
    if [ "$status" = "enabled" ]; then
      echo "CONFIG_ACTIVE"
    else
      echo "CONFIG_INACTIVE"
    fi
  `);

  t.is(result.exitCode, 0);
  t.true(result.stdout.includes("CONFIG_ACTIVE"));
});

test("integration: ScriptedTool CRUD error handling", async (t) => {
  const tool = makeCrudTool();
  const result = await tool.execute(`
    result=$(read --key missing | jq -r '.error')
    if [ "$result" = "not found" ]; then
      echo "HANDLED"
    else
      echo "MISSED"
    fi
  `);

  t.is(result.exitCode, 0);
  t.true(result.stdout.includes("HANDLED"));
});

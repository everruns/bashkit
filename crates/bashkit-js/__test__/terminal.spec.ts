import test from "ava";
import { Bash, Terminal } from "../wrapper.js";

// ============================================================================
// Terminal: interactive in-process terminal (vi, less, persistent shell)
// ============================================================================

test("terminal: low-level loop runs commands and reads screen", async (t) => {
  const term = new Terminal({ rows: 10, cols: 40 });
  t.is(await term.runUntilIdle(), "idle");
  term.send("echo hello\r");
  t.is(await term.runUntilIdle(), "idle");
  t.is(term.screenText(), "$ echo hello\nhello\n$");
  t.is(term.activity().state, "prompt");
  const [record] = term.takeTranscript();
  t.deepEqual(record, {
    command: "echo hello",
    output: "hello\n",
    exitCode: 0,
    outputTruncated: false,
  });
});

test("terminal: call edits a file in vi", async (t) => {
  const term = new Terminal();
  let out = await term.call("vi /tmp/n.txt<Enter>");
  t.is(out.activity, "running");
  t.is(out.running_command, "vi /tmp/n.txt");
  t.true(out.full_screen);
  out = await term.call("ihello<Esc>:wq<Enter>");
  t.is(out.activity, "prompt");
  t.is(out.commands[0].exit_code, 0);
  t.is((await term.readFile("/tmp/n.txt")).toString(), "hello\n");
});

test("terminal: history keeps scrolled-off output", async (t) => {
  const term = new Terminal({ rows: 5, cols: 30 });
  await term.call("seq 1 40<Enter>");
  t.true(term.historyText().includes("\n1\n2\n"));
  t.false(term.screenText().includes("\n1\n"));
});

test("terminal: timeout reports running command and Ctrl-C interrupts", async (t) => {
  const term = new Terminal();
  term.send("sleep 0.5; echo done\r");
  t.is(await term.runUntilIdle(30), "timeout");
  const activity = term.activity();
  t.is(activity.state, "running");
  t.is(activity.command, "sleep 0.5; echo done");
  // Sync send lands while the command still runs; Ctrl-C drops it at once,
  // so `echo done` never runs.
  term.send("\x03");
  t.is(await term.runUntilIdle(5000), "idle");
  const [record] = term.takeTranscript();
  t.is(record.exitCode, 130);
  t.false(record.output.includes("done"));
});

test("terminal: wait_for and screen changes", async (t) => {
  const term = new Terminal();
  const out = await term.call({
    input: "for i in 1 2 3; do echo tick$i; sleep 1; done<Enter>",
    wait_ms: 10000,
    wait_for: "tick2",
    screen: "changes",
  });
  t.is(out.matched, true);
  t.is(out.activity, "running");
  t.is(out.screen, undefined);
  t.true(out.screen_changes!.some((c) => c.text === "tick2"));
  const after = await term.call({ input: "<C-c>", screen: "changes" });
  t.is(after.activity, "prompt");
  t.deepEqual(
    after.screen_changes!.slice(-2).map((c) => c.text),
    ["^C", "$"],
  );
});

test("terminal: exit and exitCode", async (t) => {
  const term = new Terminal();
  const out = await term.call("exit 3<Enter>");
  t.is(out.activity, "exited");
  t.is(out.exit_code, 3);
  t.is(term.exitCode, 3);
  t.is(await term.runUntilIdle(), "exited");
});

test("terminal: options apply to the session", async (t) => {
  const term = new Terminal({
    username: "ada",
    cwd: "/tmp",
    env: { GREETING: "hi" },
  });
  const out = await term.call("echo $GREETING $(whoami) $PWD<Enter>");
  t.is(out.commands[0].output, "hi ada /tmp\n");
});

test("terminal: send accepts Buffer and raw output drains", async (t) => {
  const term = new Terminal();
  term.send(Buffer.from("printf 'a\\nb\\n'\r"));
  await term.runUntilIdle();
  t.true(term.takeOutput().toString().includes("a\r\nb\r\n"));
  t.is(term.takeOutput().length, 0);
});

test("terminal: resize and size", async (t) => {
  const term = new Terminal({ rows: 24, cols: 80 });
  term.resize(30, 100);
  t.deepEqual(term.size(), [30, 100]);
  const out = await term.call("echo $COLUMNS $LINES<Enter>");
  t.is(out.commands[0].output, "100 30\n");
});

test("terminal: tool metadata", (t) => {
  const term = new Terminal();
  const def = term.toolDefinition();
  t.is(def.function.name, "terminal");
  t.truthy(def.function.parameters.properties.input);
  t.true(term.systemPrompt().startsWith("terminal:"));
});

test("terminal: oversized input is rejected", async (t) => {
  const term = new Terminal();
  await t.throwsAsync(() => term.call("a".repeat(64 * 1024 + 1)));
});

test("terminal: pagers stay non-interactive outside the terminal", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("seq 1 3 | less; vi /tmp/x");
  t.is(result.stdout, "1\n2\n3\n");
  t.is(result.exitCode, 1);
});

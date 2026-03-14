// In-process just-bash benchmark runner
// Protocol: read JSON lines from stdin, write JSON lines to stdout
// Each request: {"script": "..."}
// Each response: {"stdout": "...", "stderr": "...", "exitCode": 0}
// Sends {"ready": true} on startup

let JustBash;
try {
  // Try global install
  const mod = require("/opt/node22/lib/node_modules/just-bash/dist/bundle/index.cjs");
  JustBash = mod.Bash;
} catch {
  try {
    const mod = require("just-bash");
    JustBash = mod.Bash;
  } catch {
    process.stderr.write("just-bash not available\n");
    process.exit(1);
  }
}

const readline = require("readline");
const rl = readline.createInterface({ input: process.stdin });

// Signal ready
process.stdout.write(JSON.stringify({ ready: true }) + "\n");

rl.on("line", async (line) => {
  try {
    const { script } = JSON.parse(line);
    const bash = new JustBash();
    const result = await bash.exec(script);
    process.stdout.write(
      JSON.stringify({
        stdout: result.stdout || "",
        stderr: result.stderr || "",
        exitCode: result.exitCode ?? 0,
      }) + "\n"
    );
  } catch (e) {
    process.stdout.write(
      JSON.stringify({
        stdout: "",
        stderr: e.message,
        exitCode: 1,
      }) + "\n"
    );
  }
});

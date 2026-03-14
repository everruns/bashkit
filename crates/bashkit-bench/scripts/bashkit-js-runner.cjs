// In-process bashkit-js benchmark runner
// Protocol: read JSON lines from stdin, write JSON lines to stdout
// Each request: {"script": "..."}
// Each response: {"stdout": "...", "stderr": "...", "exitCode": 0}
// Sends {"ready": true} on startup

const path = require("path");

// Try to load bashkit-js from the local build first, then from npm
let Bash;
const localPath = path.resolve(__dirname, "../../bashkit-js/index.cjs");
try {
  const mod = require(localPath);
  Bash = mod.Bash;
} catch {
  try {
    const mod = require("@everruns/bashkit");
    Bash = mod.Bash;
  } catch {
    process.stderr.write("bashkit-js not available (tried " + localPath + ")\n");
    process.exit(1);
  }
}

const readline = require("readline");
const rl = readline.createInterface({ input: process.stdin });

// Signal ready
process.stdout.write(JSON.stringify({ ready: true }) + "\n");

rl.on("line", (line) => {
  try {
    const { script } = JSON.parse(line);
    const bash = new Bash();
    const result = bash.executeSync(script);
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

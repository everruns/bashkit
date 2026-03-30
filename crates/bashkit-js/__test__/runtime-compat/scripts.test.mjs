// Real-world script patterns: JSON pipelines, data transforms, heredoc config,
// sequential commands, large output.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash } from "./_setup.mjs";

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

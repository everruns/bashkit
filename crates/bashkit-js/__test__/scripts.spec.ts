import test from "ava";
import { Bash, BashTool } from "../wrapper.js";

// ============================================================================
// Real-world script patterns
// ============================================================================

test("script: count lines in file", (t) => {
  const bash = new Bash();
  bash.executeSync('echo -e "a\\nb\\nc\\nd\\ne" > /tmp/lines.txt');
  const r = bash.executeSync("wc -l < /tmp/lines.txt");
  t.is(r.stdout.trim(), "5");
});

test("script: find and replace in file", (t) => {
  const bash = new Bash();
  bash.executeSync('echo "Hello World" > /tmp/replace.txt');
  bash.executeSync("sed -i 's/World/Earth/' /tmp/replace.txt");
  t.is(bash.executeSync("cat /tmp/replace.txt").stdout.trim(), "Hello Earth");
});

test("script: extract unique values", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(
    'echo -e "apple\\nbanana\\napple\\ncherry\\nbanana" | sort -u'
  );
  t.is(r.stdout.trim(), "apple\nbanana\ncherry");
});

test("script: JSON processing pipeline", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    echo '[{"name":"alice","age":30},{"name":"bob","age":25}]' | \
      jq -r '.[] | select(.age > 28) | .name'
  `);
  t.is(r.stdout.trim(), "alice");
});

test("script: create directory tree and verify", (t) => {
  const bash = new Bash();
  bash.executeSync("mkdir -p /tmp/project/{src,lib,test}");
  bash.executeSync("touch /tmp/project/src/main.sh");
  bash.executeSync("touch /tmp/project/test/test.sh");
  const r = bash.executeSync("ls /tmp/project/src/");
  t.true(r.stdout.includes("main.sh"));
});

test("script: config file generator", (t) => {
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
  t.is(config.name, "myapp");
  t.is(config.port, 8080);
});

test("script: loop with accumulator", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    SUM=0
    for n in 1 2 3 4 5; do
      SUM=$((SUM + n))
    done
    echo $SUM
  `);
  t.is(r.stdout.trim(), "15");
});

test("script: data transformation pipeline", (t) => {
  const bash = new Bash();
  bash.executeSync(
    'echo -e "Alice,30\\nBob,25\\nCharlie,35" > /tmp/data.csv'
  );
  const r = bash.executeSync(
    "cat /tmp/data.csv | sort -t, -k2 -n | head -1 | cut -d, -f1"
  );
  t.is(r.stdout.trim(), "Bob");
});

test("script: error handling with ||", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(
    'cat /nonexistent/file 2>/dev/null || echo "fallback"'
  );
  t.is(r.stdout.trim(), "fallback");
});

test("script: conditional file creation", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    FILE=/tmp/conditional.txt
    if [ ! -f "$FILE" ]; then
      echo "created" > "$FILE"
    fi
    cat "$FILE"
  `);
  t.is(r.stdout.trim(), "created");
});

// ============================================================================
// Multiline scripts
// ============================================================================

test("multiline: function with multiple operations", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    process_list() {
      local items="$1"
      echo "$items" | tr ',' '\\n' | sort | while read item; do
        echo "- $item"
      done
    }
    process_list "cherry,apple,banana"
  `);
  const lines = r.stdout.trim().split("\n");
  t.is(lines[0], "- apple");
  t.is(lines[1], "- banana");
  t.is(lines[2], "- cherry");
});

test("multiline: nested loops", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    for i in 1 2; do
      for j in a b; do
        echo -n "$i$j "
      done
    done
  `);
  t.is(r.stdout.trim(), "1a 1b 2a 2b");
});

test("multiline: while read loop", (t) => {
  const bash = new Bash();
  const r = bash.executeSync(`
    echo -e "1:one\\n2:two\\n3:three" | while IFS=: read num word; do
      echo "$word=$num"
    done
  `);
  t.is(r.stdout.trim(), "one=1\ntwo=2\nthree=3");
});

// ============================================================================
// BashTool — real-world LLM tool usage patterns
// ============================================================================

test("BashTool: LLM-style single command", (t) => {
  const tool = new BashTool();
  const r = tool.executeSync("echo 'Hello from the AI agent'");
  t.is(r.exit_code, 0);
  t.true(r.stdout.includes("Hello from the AI agent"));
});

test("BashTool: LLM-style multi-step script", (t) => {
  const tool = new BashTool();
  const r = tool.executeSync(`
    mkdir -p /tmp/workspace
    echo '{"status": "ok"}' > /tmp/workspace/result.json
    cat /tmp/workspace/result.json | jq -r '.status'
  `);
  t.is(r.stdout.trim(), "ok");
});

test("BashTool: LLM-style data analysis", (t) => {
  const tool = new BashTool();
  const r = tool.executeSync(`
    echo -e "2024-01-01,100\\n2024-01-02,200\\n2024-01-03,150" > /tmp/sales.csv
    TOTAL=$(awk -F, '{sum+=$2} END {print sum}' /tmp/sales.csv)
    COUNT=$(wc -l < /tmp/sales.csv)
    echo "Total: $TOTAL, Count: $COUNT"
  `);
  t.true(r.stdout.includes("Total: 450"));
  t.true(r.stdout.includes("Count: 3"));
});

test("BashTool: sequential calls build state", (t) => {
  const tool = new BashTool();
  tool.executeSync("mkdir -p /tmp/project && cd /tmp/project");
  tool.executeSync('echo "# My Project" > /tmp/project/README.md');
  tool.executeSync('echo "fn main() {}" > /tmp/project/main.rs');
  const r = tool.executeSync("ls /tmp/project/");
  t.true(r.stdout.includes("README.md"));
  t.true(r.stdout.includes("main.rs"));
});

// ============================================================================
// Stress / edge cases
// ============================================================================

test("many sequential commands", (t) => {
  const bash = new Bash();
  for (let i = 0; i < 50; i++) {
    bash.executeSync(`echo ${i}`);
  }
  t.is(bash.executeSync("echo done").stdout.trim(), "done");
});

test("large output", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("seq 1 1000");
  const lines = r.stdout.trim().split("\n");
  t.is(lines.length, 1000);
  t.is(lines[0], "1");
  t.is(lines[999], "1000");
});

test("empty stdin pipe", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo '' | grep 'x'");
  t.not(r.exit_code, 0);
});

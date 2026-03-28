// Builtin commands: grep, sed, awk, sort, uniq, tr, cut, head, tail, wc,
// base64, jq, md5sum, sha256sum, seq, printf, date, export/unset.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash } from "./_setup.mjs";

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
      bash.executeSync("echo -n 'hello' | md5sum").stdout.includes(
        "5d41402abc4b2a76b9719d911017c592",
      ),
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

// Lazy filesystems: FileSystem.lazy loads listings and contents on demand.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash, FileSystem } from "./_setup.mjs";

function source(calls) {
  return {
    readDir(path) {
      calls.push(`readDir ${path}`);
      if (path === "/") {
        return [
          { name: "a.txt", type: "file", size: 2 },
          { name: "sub", type: "dir" },
        ];
      }
      if (path === "/sub") return [{ name: "b.txt", type: "file" }];
      return null;
    },
    async readFile(path) {
      calls.push(`readFile ${path}`);
      return path === "/a.txt" ? "A\n" : path === "/sub/b.txt" ? "B\n" : null;
    },
  };
}

describe("lazy filesystem", () => {
  it("loads only what a script touches, once", async () => {
    const calls = [];
    const bash = new Bash();
    bash.mount("/repo", FileSystem.lazy(source(calls)));
    assert.deepEqual(calls, []);
    const r = await bash.execute("cat /repo/a.txt; cat /repo/a.txt");
    assert.equal(r.stdout, "A\nA\n");
    assert.deepEqual(calls, ["readDir /", "readFile /a.txt"]);
  });

  it("walks directories on demand", async () => {
    const bash = new Bash();
    bash.mount("/repo", FileSystem.lazy(source([])));
    const r = await bash.execute("find /repo -type f | sort");
    assert.equal(r.stdout, "/repo/a.txt\n/repo/sub/b.txt\n");
  });

  it("is read-only", async () => {
    const bash = new Bash();
    bash.mount("/repo", FileSystem.lazy(source([])));
    const r = await bash.execute("echo x > /repo/new.txt");
    assert.notEqual(r.exitCode, 0);
    assert.match(r.stderr, /read-only/);
  });

  it("fails fast under executeSync", () => {
    const bash = new Bash();
    bash.mount("/repo", FileSystem.lazy(source([])));
    const r = bash.executeSync("cat /repo/a.txt");
    assert.notEqual(r.exitCode, 0);
    assert.match(r.stderr, /require async access/);
  });

  it("cannot be exported with toExternal()", () => {
    const fs = FileSystem.lazy(source([]));
    assert.throws(() => fs.toExternal(), /cannot be exported/);
  });
});

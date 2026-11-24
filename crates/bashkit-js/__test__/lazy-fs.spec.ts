import test from "ava";
import { Bash, BashTool, FileSystem, type LazyDirEntry } from "../wrapper.js";

// ============================================================================
// FileSystem.lazy — layout and contents loaded on demand
// ============================================================================

type Tree = { [name: string]: string | Tree };

const TREE: Tree = {
  "README.md": "hello\n",
  src: {
    "index.ts": "export const x = 1; // TODO\n",
    deep: { "leaf.txt": "leaf\n" },
  },
  docs: { "guide.md": "guide TODO\n" },
};

function lookup(path: string): string | Tree | undefined {
  let node: string | Tree | undefined = TREE;
  for (const part of path.split("/").filter(Boolean)) {
    if (typeof node !== "object") return undefined;
    node = node[part];
  }
  return node;
}

/** A lazy source over TREE that records every host call. */
function recordingSource(options: { async?: boolean } = {}) {
  const calls: string[] = [];
  const wrap = <T>(v: T) => (options.async ? Promise.resolve(v) : v);
  const source = {
    readDir(path: string) {
      calls.push(`readDir ${path}`);
      const node = lookup(path);
      if (typeof node !== "object") return wrap(null);
      const entries: LazyDirEntry[] = Object.entries(node).map(([name, v]) =>
        typeof v === "string"
          ? { name, type: "file", size: Buffer.byteLength(v) }
          : { name, type: "dir" },
      );
      return wrap(entries);
    },
    readFile(path: string) {
      calls.push(`readFile ${path}`);
      const node = lookup(path);
      return wrap(typeof node === "string" ? node : null);
    },
  };
  return { source, calls };
}

test("lazy: nothing is loaded until a script touches it", async (t) => {
  const { source, calls } = recordingSource();
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source));
  t.deepEqual(calls, []);

  const r = await bash.execute("cat /repo/README.md");
  t.is(r.stdout, "hello\n");
  t.deepEqual(calls, ["readDir /", "readFile /README.md"]);
});

test("lazy: listing a directory only loads that directory", async (t) => {
  const { source, calls } = recordingSource();
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source));

  const r = await bash.execute("ls /repo/src");
  t.is(r.stdout, "deep\nindex.ts\n");
  t.deepEqual(calls, ["readDir /", "readDir /src"]);
});

test("lazy: stat and ls -l use declared sizes without reading files", async (t) => {
  const { source, calls } = recordingSource();
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source));

  const r = await bash.execute(
    "ls -l /repo/README.md; test -f /repo/src/index.ts && echo ok",
  );
  t.regex(r.stdout, /\s6\s/);
  t.regex(r.stdout, /ok/);
  t.false(calls.some((c) => c.startsWith("readFile")));
});

test("lazy: results are cached across commands and executions", async (t) => {
  const { source, calls } = recordingSource({ async: true });
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source));

  await bash.execute("cat /repo/src/index.ts; cat /repo/src/index.ts");
  await bash.execute("ls /repo/src; cat /repo/src/index.ts");
  t.deepEqual(calls, ["readDir /", "readDir /src", "readFile /src/index.ts"]);
});

test("lazy: recursive tools walk the tree on demand", async (t) => {
  const { source } = recordingSource({ async: true });
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source));

  const find = await bash.execute("find /repo -type f | sort");
  t.is(
    find.stdout,
    "/repo/README.md\n/repo/docs/guide.md\n/repo/src/deep/leaf.txt\n/repo/src/index.ts\n",
  );
  const grep = await bash.execute("grep -rl TODO /repo | sort");
  t.is(grep.stdout, "/repo/docs/guide.md\n/repo/src/index.ts\n");
});

test("lazy: unknown paths are not found and never reach readFile", async (t) => {
  const { source, calls } = recordingSource();
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source));

  const r = await bash.execute("cat /repo/nope.txt; cat /repo/nodir/x");
  t.not(r.exitCode, 0);
  t.is(
    r.stderr,
    "cat: /repo/nope.txt: No such file or directory\ncat: /repo/nodir/x: No such file or directory\n",
  );
  t.false(calls.some((c) => c.startsWith("readFile")));
  t.false(calls.includes("readDir /nodir"));
});

test("lazy: Uint8Array content round-trips as bytes", async (t) => {
  const bytes = new Uint8Array([0, 1, 2, 250, 255]);
  const bash = new Bash();
  bash.mount(
    "/bin-data",
    FileSystem.lazy({
      readDir: (p) => (p === "/" ? [{ name: "blob", type: "file" }] : null),
      readFile: () => bytes,
    }),
  );
  const r = await bash.execute("od -An -tu1 /bin-data/blob | tr -s ' '");
  t.is(r.stdout.trim(), "0 1 2 250 255");
});

test("lazy: read-only by default", async (t) => {
  const { source } = recordingSource();
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source));
  const r = await bash.execute("echo x > /repo/new.txt");
  t.not(r.exitCode, 0);
  t.regex(r.stderr, /read-only/i);
});

test("lazy: callback errors surface as command errors", async (t) => {
  const bash = new Bash();
  bash.mount(
    "/broken",
    FileSystem.lazy({
      readDir: () => [{ name: "f", type: "file" }],
      readFile: async () => {
        throw new Error("backend down");
      },
    }),
  );
  const r = await bash.execute("cat /broken/f");
  t.not(r.exitCode, 0);
  t.regex(r.stderr, /backend down/);
});

test("lazy: invalid entry names are rejected", async (t) => {
  const bash = new Bash();
  bash.mount(
    "/evil",
    FileSystem.lazy({
      readDir: () => [{ name: "../escape", type: "file" }],
      readFile: () => "x",
    }),
  );
  const r = await bash.execute("ls /evil");
  t.not(r.exitCode, 0);
  t.regex(r.stderr, /invalid entry name/);
});

test("lazy: sync access fails fast instead of deadlocking", (t) => {
  const { source } = recordingSource();
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source));
  const r = bash.executeSync("cat /repo/README.md");
  t.not(r.exitCode, 0);
  t.regex(r.stderr, /require async access/);
  t.throws(() => bash.readFile("/repo/README.md"), {
    message: /require async access/,
  });
});

test("lazy: requires both callbacks", (t) => {
  t.throws(() => FileSystem.lazy({ readDir: () => [] } as never), {
    instanceOf: TypeError,
  });
});

// ============================================================================
// Review regressions
// ============================================================================

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const never = () => new Promise<never>(() => {});
const flat = (readFile: (p: string) => unknown, n = 20) =>
  FileSystem.lazy({
    readDir: (p) =>
      p === "/"
        ? Array.from({ length: n }, (_, i) => ({
            name: `f${i}`,
            type: "file" as const,
          }))
        : null,
    readFile: readFile as never,
  });

test("lazy: error kinds match the in-memory filesystem", async (t) => {
  const bash = new Bash();
  bash.mount(
    "/r",
    FileSystem.lazy({
      readDir: (p) =>
        p === "/"
          ? [
              { name: "f", type: "file" },
              { name: "d", type: "dir" },
            ]
          : [],
      readFile: () => "x",
    }),
  );
  const r = await bash.execute(
    "cat /r/f/x; ls /r/f/x; cat /r/d; test -e /r/f/x || echo absent",
  );
  t.is(
    r.stderr,
    [
      "cat: /r/f/x: Not a directory",
      "ls: cannot access '/r/f/x': Not a directory",
      "cat: /r/d: Is a directory",
      "",
    ].join("\n"),
  );
  t.is(r.stdout, "absent\n");
});

test("lazy: callback errors are sanitized", async (t) => {
  const bash = new Bash();
  bash.mount(
    "/r",
    FileSystem.lazy({
      readDir: () => [{ name: "f", type: "file" }],
      readFile: () => {
        const e = new Error(
          "connect ECONNREFUSED 10.0.0.5:5432 at /home/host/secret/app.js " +
            "x".repeat(400),
        );
        throw e;
      },
    }),
  );
  const r = await bash.execute("cat /r/f");
  t.not(r.exitCode, 0);
  t.false(r.stderr.includes("10.0.0.5"));
  t.false(r.stderr.includes("/home/host"));
  t.false(r.stderr.includes("io error"));
  t.false(r.stderr.includes("GenericFailure"));
  t.regex(
    r.stderr,
    /^cat: \/r\/f: connect ECONNREFUSED <addr> at <path> x+\n$/,
  );
  t.true(r.stderr.length < 300);
});

test.serial("lazy: timeoutMs bounds a pending callback", async (t) => {
  const bash = new Bash({ timeoutMs: 300 });
  bash.mount("/r", flat(never));
  const started = Date.now();
  const r = await bash.execute("cat /r/f1");
  t.is(r.exitCode, 124);
  t.true(Date.now() - started < 3000);
});

test.serial(
  "lazy: pending callbacks do not starve unrelated executions",
  async (t) => {
    const fs = flat(never);
    for (let i = 0; i < 4; i++) {
      const b = new Bash({ timeoutMs: 2000 });
      b.mount("/r", fs);
      void b.execute(`cat /r/f${i}`);
    }
    const r = await Promise.race([
      new Bash().execute("echo hi"),
      sleep(3000).then(() => ({ stdout: "starved" })),
    ]);
    t.is(r.stdout, "hi\n");
  },
);

test.serial("lazy: slow callbacks run concurrently", async (t) => {
  const fs = flat(async (p: string) => {
    await sleep(200);
    return p;
  });
  const started = Date.now();
  await Promise.all(
    Array.from({ length: 8 }, (_, i) => {
      const b = new Bash();
      b.mount("/r", fs);
      return b.execute(`cat /r/f${i}`);
    }),
  );
  t.true(Date.now() - started < 1600, `took ${Date.now() - started}ms`);
});

test.serial("lazy: concurrent first reads share one host call", async (t) => {
  let dirCalls = 0;
  let fileCalls = 0;
  const fs = FileSystem.lazy({
    readDir: async () => {
      dirCalls++;
      await sleep(50);
      return [{ name: "f", type: "file" }];
    },
    readFile: async () => {
      fileCalls++;
      await sleep(50);
      return "F\n";
    },
  });
  const results = await Promise.all(
    Array.from({ length: 5 }, () => {
      const b = new Bash();
      b.mount("/r", fs);
      return b.execute("cat /r/f");
    }),
  );
  t.true(results.every((r) => r.stdout === "F\n"));
  t.is(dirCalls, 1);
  t.is(fileCalls, 1);
});

test("lazy: missing files are cached negatively", async (t) => {
  const { source, calls } = recordingSource();
  const gone = {
    ...source,
    readDir: (p: string) =>
      p === "/" ? [{ name: "gone", type: "file" as const }] : null,
  };
  const bash = new Bash();
  bash.mount("/r", FileSystem.lazy(gone));
  await bash.execute("cat /r/gone; cat /r/gone");
  t.is(calls.filter((c) => c === "readFile /gone").length, 1);
});

test("lazy: oversized files are rejected without fetching", async (t) => {
  let fetched = 0;
  const bash = new Bash();
  bash.mount(
    "/r",
    FileSystem.lazy({
      readDir: () => [{ name: "big", type: "file", size: 50_000_000 }],
      readFile: () => {
        fetched++;
        return "x";
      },
    }),
  );
  const r = await bash.execute("cat /r/big; cat /r/big");
  t.is(r.stderr, "cat: /r/big: File too large\ncat: /r/big: File too large\n");
  t.is(fetched, 0);
});

test("lazy: unsafe entry names are rejected", async (t) => {
  for (const name of ["a\nb", "evil\u202Etxt", "x".repeat(300)]) {
    const bash = new Bash();
    bash.mount(
      "/r",
      FileSystem.lazy({
        readDir: () => [{ name, type: "file" }],
        readFile: () => "",
      }),
    );
    const r = await bash.execute("ls /r");
    t.not(r.exitCode, 0, JSON.stringify(name.slice(0, 10)));
    t.regex(r.stderr, /invalid entry name/);
  }
});

test("lazy: out-of-range optional fields fall back to defaults", async (t) => {
  const bash = new Bash();
  bash.mount(
    "/r",
    FileSystem.lazy({
      readDir: () => [
        { name: "a", type: "file", size: -1, mode: 1.5, modified: 1e300 },
        { name: "b", type: "file", size: 1.5 },
      ],
      readFile: () => "ok",
    }),
  );
  const r = await bash.execute("ls /r");
  t.is(r.stdout, "a\nb\n");
});

test("lazy: lone surrogates in text are transported safely", async (t) => {
  const bash = new Bash();
  bash.mount(
    "/r",
    FileSystem.lazy({
      readDir: () => [{ name: "f", type: "file" }],
      readFile: () => "a\ud800b",
    }),
  );
  const r = await bash.execute("cat /r/f");
  t.is(r.exitCode, 0);
  t.is(r.stdout, "a\ufffdb");
});

test.serial(
  "lazy: unrelated sync calls do not break async reads",
  async (t) => {
    const fs = flat(async () => {
      await sleep(2);
      return "x\n";
    }, 30);
    const a = new Bash();
    a.mount("/r", fs);
    const other = new Bash();
    const timer = setInterval(() => other.executeSync("true"), 1);
    try {
      const r = await a.execute(
        "for i in $(seq 0 29); do cat /r/f$i; done | wc -l",
      );
      t.is(r.stdout.trim(), "30");
      t.is(r.stderr, "");
    } finally {
      clearInterval(timer);
    }
  },
);

test("lazy: BashTool mounts lazy filesystems too", async (t) => {
  const { source } = recordingSource();
  const tool = new BashTool();
  tool.mount("/repo", FileSystem.lazy(source));
  const r = await tool.execute("cat /repo/README.md");
  t.is(r.stdout, "hello\n");
});

test("lazy: unmount and reset keep working", async (t) => {
  const { source } = recordingSource();
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source));
  t.is((await bash.execute("cat /repo/README.md")).stdout, "hello\n");
  bash.reset();
  t.is((await bash.execute("cat /repo/README.md")).stdout, "hello\n");
  bash.unmount("/repo");
  t.not((await bash.execute("cat /repo/README.md")).exitCode, 0);
});

test("lazy: handles cannot be exported through the C ABI", (t) => {
  const { source } = recordingSource();
  const fs = FileSystem.lazy(source);
  t.throws(() => fs.toExternal(), { message: /cannot be exported/ });
  const bash = new Bash();
  const foreign = { toExternal: () => fs.toExternal() };
  t.throws(() => bash.mount("/r", foreign as never), {
    message: /cannot be exported/,
  });
});

test.serial(
  "lazy: sync access from a helper thread fails instead of hanging",
  (t) => {
    // sqlite's VFS backend does its I/O on a helper thread, which the
    // same-thread check cannot see; the bounded wait must end the call.
    const bash = new Bash({ sqlite: true });
    bash.mount(
      "/r",
      FileSystem.lazy({
        readDir: () => [{ name: "d.db", type: "file" }],
        readFile: () => new Uint8Array(0),
      }),
    );
    for (let i = 0; i < 2; i++) {
      const started = Date.now();
      bash.executeSync("sqlite3 -backend vfs /r/d.db 'select 1'");
      t.true(Date.now() - started < 10_000, `took ${Date.now() - started}ms`);
    }
  },
);

test("lazy: oversized undeclared files stay too large and report their size", async (t) => {
  let fetched = 0;
  const big = "x".repeat(10_000_001);
  const bash = new Bash();
  bash.mount(
    "/r",
    FileSystem.lazy({
      readDir: () => [{ name: "big", type: "file" }],
      readFile: () => {
        fetched++;
        return big;
      },
    }),
  );
  const r = await bash.execute(
    "cat /r/big; cat /r/big; wc -c < /r/big; stat -c %s /r/big",
  );
  t.is(
    r.stderr.split("\n").filter((l) => l.includes("File too large")).length,
    3,
    r.stderr,
  );
  t.is(r.stdout.trim().split("\n").pop(), "10000001");
  t.is(fetched, 1);
});

test("lazy: other filesystem mounts keep their existing semantics", async (t) => {
  // Cyclic live mounts go through the ABI path, which reports zero usage.
  const a = new Bash();
  const b = new Bash();
  a.mount("/b", b.fs());
  b.mount("/a", a.fs());
  const r = await a.execute("df > /dev/null; echo done");
  t.is(r.stdout, "done\n");
});

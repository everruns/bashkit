import test from "ava";
import { Bash, FileSystem, type LazyDirEntry } from "../wrapper.js";

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
  t.regex(r.stderr, /No such file or directory/i);
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

test("lazy: writable option keeps writes in memory", async (t) => {
  const { source, calls } = recordingSource();
  const bash = new Bash();
  bash.mount("/repo", FileSystem.lazy(source, { writable: true }));
  const r = await bash.execute(
    "echo changed > /repo/README.md && echo new > /repo/new.txt && cat /repo/README.md /repo/new.txt",
  );
  t.is(r.stdout, "changed\nnew\n");
  t.false(calls.some((c) => /write/i.test(c)));
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

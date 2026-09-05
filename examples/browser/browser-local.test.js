import assert from "node:assert/strict";
import test from "node:test";

import { browserLocal } from "./browser-local.js";

class MemoryStorage {
  #values = new Map();

  getItem(key) {
    return this.#values.get(key) ?? null;
  }

  setItem(key, value) {
    this.#values.set(key, value);
  }

  removeItem(key) {
    this.#values.delete(key);
  }
}

function fakeFs(tree) {
  return {
    exists(path) { return Object.hasOwn(tree, path); },
    ls(path) {
      const value = tree[path];
      if (!Array.isArray(value)) throw new Error("not a directory");
      return value;
    },
    readFile(path) {
      const value = tree[path];
      if (typeof value !== "string") throw new Error("not a file");
      return value;
    },
  };
}

test("loads persisted files and overlays them on defaults", () => {
  const storage = new MemoryStorage();
  storage.setItem("bashkit:fs", JSON.stringify({
    version: 1,
    files: {
      "/home/user/welcome.txt": "edited\n",
      "/home/user/notes/todo.txt": "ship it\n",
    },
  }));

  const backend = browserLocal({ storage });

  assert.deepEqual(backend.load({
    "/home/user/welcome.txt": "default\n",
    "/home/user/demo.sh": "echo demo\n",
  }), {
    "/home/user/welcome.txt": "edited\n",
    "/home/user/demo.sh": "echo demo\n",
    "/home/user/notes/todo.txt": "ship it\n",
  });
});

test("saves nested files and replaces deleted persisted files", () => {
  const storage = new MemoryStorage();
  storage.setItem("bashkit:fs", JSON.stringify({
    version: 1,
    files: { "/home/user/deleted.txt": "old\n" },
  }));
  const backend = browserLocal({ storage });

  backend.save(fakeFs({
    "/home/user": ["notes", "plain.txt"],
    "/home/user/notes": ["todo.txt"],
    "/home/user/notes/todo.txt": "ship it\n",
    "/home/user/plain.txt": "hello\n",
  }));

  assert.deepEqual(JSON.parse(storage.getItem("bashkit:fs")), {
    version: 1,
    files: {
      "/home/user/notes/todo.txt": "ship it\n",
      "/home/user/plain.txt": "hello\n",
    },
  });
});

test("ignores malformed or incompatible local storage data", () => {
  const storage = new MemoryStorage();
  const backend = browserLocal({ storage });

  storage.setItem("bashkit:fs", "not json");
  assert.deepEqual(backend.load({ "/default": "value" }), { "/default": "value" });

  storage.setItem("bashkit:fs", JSON.stringify({ version: 2, files: { "/bad": "data" } }));
  assert.deepEqual(backend.load({ "/default": "value" }), { "/default": "value" });
});

test("clears persisted files when the storage root was removed", () => {
  const storage = new MemoryStorage();
  storage.setItem("bashkit:fs", JSON.stringify({
    version: 1,
    files: { "/home/user/deleted.txt": "old\n" },
  }));

  assert.equal(browserLocal({ storage }).save(fakeFs({})), true);
  assert.deepEqual(JSON.parse(storage.getItem("bashkit:fs")), {
    version: 1,
    files: {},
  });
});

test("reports localStorage write failures without throwing", () => {
  const storage = new MemoryStorage();
  storage.setItem = () => { throw new Error("quota exceeded"); };

  assert.equal(browserLocal({ storage }).save(fakeFs({
    "/home/user": ["note.txt"],
    "/home/user/note.txt": "hello\n",
  })), false);
});

test("tolerates blocked access to the global localStorage", () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    get() { throw new DOMException("blocked", "SecurityError"); },
  });

  try {
    const backend = browserLocal();
    assert.deepEqual(backend.load({ "/home/user/welcome.txt": "hello\n" }), {
      "/home/user/welcome.txt": "hello\n",
    });
    assert.equal(backend.save(fakeFs({})), false);
    assert.doesNotThrow(() => backend.clear());
  } finally {
    if (descriptor) Object.defineProperty(globalThis, "localStorage", descriptor);
    else delete globalThis.localStorage;
  }
});

test("clear removes the persisted filesystem", () => {
  const storage = new MemoryStorage();
  storage.setItem("bashkit:fs", "saved");

  browserLocal({ storage }).clear();

  assert.equal(storage.getItem("bashkit:fs"), null);
});

test("rejects unsafe persisted paths", () => {
  const storage = new MemoryStorage();
  storage.setItem("bashkit:fs", JSON.stringify({
    version: 1,
    files: {
      "/home/user/ok.txt": "ok",
      "/home/user/../outside.txt": "bad",
      "/tmp/outside.txt": "bad",
    },
  }));

  assert.deepEqual(browserLocal({ storage }).load(), {
    "/home/user/ok.txt": "ok",
  });
});

for (const unreadable of ["binary", "directory"]) {
  test(`preserves previous snapshot when ${unreadable} traversal fails`, () => {
    const storage = new MemoryStorage();
    const previous = JSON.stringify({ version: 1, files: { "/home/user/old.txt": "keep" } });
    storage.setItem("bashkit:fs", previous);
    const fs = fakeFs({
      "/home/user": ["first.txt", "unreadable"],
      "/home/user/first.txt": "new partial data",
      "/home/user/unreadable": unreadable === "binary" ? new Uint8Array([255]) : ["child"],
    });
    assert.equal(browserLocal({ storage }).save(fs), false);
    assert.equal(storage.getItem("bashkit:fs"), previous);
  });
}

test("preserves previous snapshot when the existing root cannot be listed", () => {
  const storage = new MemoryStorage();
  storage.setItem("bashkit:fs", "previous snapshot");
  const fs = fakeFs({ "/home/user": new Error("permission denied") });
  assert.equal(browserLocal({ storage }).save(fs), false);
  assert.equal(storage.getItem("bashkit:fs"), "previous snapshot");
});

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

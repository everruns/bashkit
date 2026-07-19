// browserLocal persists the browser example's /home/user tree in localStorage.
const DEFAULT_KEY = "bashkit:fs";
const DEFAULT_ROOT = "/home/user";
const FORMAT_VERSION = 1;

function isStoredPath(path, root) {
  if (typeof path !== "string" || typeof root !== "string") return false;
  if (path !== root && !path.startsWith(`${root}/`)) return false;
  return !path.split("/").includes("..");
}

function joinPath(parent, child) {
  return parent === "/" ? `/${child}` : `${parent}/${child}`;
}

function readStoredFiles(storage, key, root) {
  try {
    const value = JSON.parse(storage.getItem(key));
    if (value?.version !== FORMAT_VERSION || !value.files || Array.isArray(value.files)) {
      return {};
    }

    return Object.fromEntries(Object.entries(value.files).filter(
      ([path, contents]) => isStoredPath(path, root) && typeof contents === "string",
    ));
  } catch {
    return {};
  }
}

function snapshotDirectory(fs, directory, files) {
  for (const name of fs.ls(directory)) {
    const path = joinPath(directory, name);
    try {
      files[path] = fs.readFile(path);
    } catch {
      snapshotDirectory(fs, path, files);
    }
  }
}

/** Create a localStorage-backed persistence adapter for BashKit's browser VFS. */
export function browserLocal({
  storage = globalThis.localStorage,
  key = DEFAULT_KEY,
  root = DEFAULT_ROOT,
} = {}) {
  if (!storage) throw new Error("browserLocal requires a Storage implementation");

  return {
    load(defaultFiles = {}) {
      return { ...defaultFiles, ...readStoredFiles(storage, key, root) };
    },

    save(fs) {
      const files = {};
      try {
        snapshotDirectory(fs, root, files);
      } catch {
        // The persisted root may have been removed by the last command.
      }

      try {
        storage.setItem(key, JSON.stringify({ version: FORMAT_VERSION, files }));
        return true;
      } catch {
        // Storage can be unavailable or full; command execution should still succeed.
        return false;
      }
    },

    clear() {
      storage.removeItem(key);
    },
  };
}

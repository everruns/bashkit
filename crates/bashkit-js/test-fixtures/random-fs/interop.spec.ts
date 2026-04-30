// Kept outside the default AVA suite because release artifact jobs run `npm test` without Rust.

import test from "ava";
import { createRequire } from "node:module";
import { Bash, FileSystem } from "../../wrapper.js";

const require = createRequire(import.meta.url);
const randomFsFixture = require(".");

test("downstream random filesystem external mounts into bash", (t) => {
  const seed = 2026;
  const external = randomFsFixture.createRandomFilesystemExternal(seed);
  const imported = FileSystem.fromExternal(external);

  const bash = new Bash();
  bash.mount("/remote", imported);

  const result = bash.executeSync("cat /remote/random.txt");
  t.is(result.exitCode, 0);
  t.is(result.stdout, randomFsFixture.expectedRandomText(seed, "/random.txt"));
});

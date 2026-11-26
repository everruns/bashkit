// Temporary host directories for tests that mount the real filesystem.
//
// Decision: a RealFs mount holds an open handle to its root directory for
// its whole lifetime, so every host operation resolves beneath that handle
// (#3008, TM-ESC-035). Windows refuses to remove a directory that has an open
// handle (EBUSY), and the native object behind a JS `Bash` or `FileSystem` is
// released only when it is garbage collected, which a test cannot force.
// Cleanup therefore tolerates EBUSY on Windows; the runner's temp folder is
// discarded with the job.

import { rmSync } from "node:fs";

export function removeTempDir(dir: string): void {
  try {
    rmSync(dir, { recursive: true, force: true });
  } catch (error) {
    if (
      process.platform === "win32" &&
      (error as NodeJS.ErrnoException).code === "EBUSY"
    ) {
      return;
    }
    throw error;
  }
}

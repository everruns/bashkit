// Control flow: if/elif/else, for, while, break, continue, case, functions,
// subshells, exit codes.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash } from "./_setup.mjs";

describe("control flow", () => {
  it("if/elif/else", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      X=2
      if [ "$X" = "1" ]; then echo one
      elif [ "$X" = "2" ]; then echo two
      else echo other
      fi
    `);
    assert.equal(r.stdout.trim(), "two");
  });

  it("for loop", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync("for i in a b c; do echo $i; done").stdout.trim(),
      "a\nb\nc",
    );
  });

  it("while loop", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      i=0
      while [ $i -lt 3 ]; do echo $i; i=$((i + 1)); done
    `);
    assert.equal(r.stdout.trim(), "0\n1\n2");
  });

  it("break and continue", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync(`
        for i in 1 2 3 4 5; do
          if [ $i -eq 4 ]; then break; fi
          if [ $i -eq 2 ]; then continue; fi
          echo $i
        done
      `).stdout.trim(),
      "1\n3",
    );
  });

  it("case statement", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      FILE=image.png
      case "$FILE" in
        *.png) echo "png";;
        *.jpg) echo "jpg";;
        *) echo "other";;
      esac
    `);
    assert.equal(r.stdout.trim(), "png");
  });

  it("functions with local vars and recursion", () => {
    const bash = new Bash();
    const r = bash.executeSync(`
      factorial() {
        if [ $1 -le 1 ]; then echo 1; return; fi
        local sub=$(factorial $(($1 - 1)))
        echo $(($1 * sub))
      }
      factorial 5
    `);
    assert.equal(r.stdout.trim(), "120");
  });

  it("subshell does not leak variables", () => {
    const bash = new Bash();
    bash.executeSync("(X=inner)");
    assert.equal(bash.executeSync("echo ${X:-unset}").stdout.trim(), "unset");
  });

  it("$? captures last exit code", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("false; echo $?").stdout.trim(), "1");
  });
});

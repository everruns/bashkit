// ScriptedTool lives in @everruns/bashkit/scripted; the root keeps a
// deprecated alias of the same class during the transition.
import test from "ava";
import { ScriptedTool } from "../scripted.js";
import * as root from "../wrapper.js";

test("scripted entry: ScriptedTool runs registered tools", async (t) => {
  const tool = new ScriptedTool({ name: "api" });
  tool.addTool("greet", "Greet user", (params) => `hi ${params.name}\n`);
  const result = await tool.execute("greet --name ada");
  t.is(result.stdout, "hi ada\n");
});

test("root entry: deprecated alias is the same class", (t) => {
  t.is(root.ScriptedTool, ScriptedTool);
  t.true(new root.ScriptedTool({ name: "x" }) instanceof ScriptedTool);
});

test("scripted entry: BashError is the shared root class", async (t) => {
  const tool = new ScriptedTool({ name: "api" });
  const error = await t.throwsAsync(() => tool.executeOrThrow("exit 3"));
  t.true(error instanceof root.BashError);
});

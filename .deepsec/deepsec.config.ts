import { defineConfig } from "deepsec/config";

export default defineConfig({
  projects: [
    { id: "bashkit", root: ".." },
    // <deepsec:projects-insert-above>
  ],
});

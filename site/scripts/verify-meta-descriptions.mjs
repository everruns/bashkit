// Decision: verify the crawler-reported public pages from generated HTML,
// because Astro props and generated inventories can change the final text.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const MIN_DESCRIPTION_LENGTH = 160;
const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDir, "..");
const distRoot = path.join(siteRoot, "dist");

const checkedPages = [
  { route: "/benches/", file: "benches/index.html" },
  { route: "/builtins/", file: "builtins/index.html" },
];

const failures = [];

for (const page of checkedPages) {
  const filePath = path.join(distRoot, page.file);

  if (!existsSync(filePath)) {
    failures.push(`${page.route}: missing generated HTML at ${page.file}`);
    continue;
  }

  const html = readFileSync(filePath, "utf8");
  const description = html.match(
    /<meta\s+name="description"\s+content="([^"]*)"\s*\/?>/,
  )?.[1];

  if (!description) {
    failures.push(`${page.route}: missing meta description`);
    continue;
  }

  if (description.length < MIN_DESCRIPTION_LENGTH) {
    failures.push(
      `${page.route}: meta description is ${description.length} chars; expected at least ${MIN_DESCRIPTION_LENGTH}`,
    );
  }
}

if (failures.length > 0) {
  throw new Error(`Meta description verification failed:\n${failures.join("\n")}`);
}

console.log("Verified crawler-reported meta descriptions.");

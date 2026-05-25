// Decision: verify Content Signals in generated robots.txt so permissive AI
// usage preferences stay declared for bashkit.sh after future site edits.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDir, "..");
const robotsPath = path.join(siteRoot, "dist", "robots.txt");

if (!existsSync(robotsPath)) {
  throw new Error(`Missing robots output: ${robotsPath}`);
}

const robots = readFileSync(robotsPath, "utf8");
const signalLine = robots
  .split(/\r?\n/)
  .find((line) => line.trim().toLowerCase().startsWith("content-signal:"));

if (!signalLine) {
  throw new Error("robots.txt does not include a Content-Signal directive");
}

const preferences = new Map(
  signalLine
    .slice(signalLine.indexOf(":") + 1)
    .split(",")
    .map((entry) => entry.trim().split("="))
    .filter(([key, value]) => key && value),
);

const expectedPreferences = new Map([
  ["ai-train", "yes"],
  ["search", "yes"],
  ["ai-input", "yes"],
]);

for (const [key, expectedValue] of expectedPreferences) {
  const actualValue = preferences.get(key);

  if (actualValue !== expectedValue) {
    throw new Error(
      `robots.txt Content-Signal must include ${key}=${expectedValue}`,
    );
  }
}

console.log("Verified robots.txt Content Signals.");

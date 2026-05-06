// Decision: rustdoc-compatible examples use `# ` setup lines so doctests can
// compile while docs hide boilerplate. Astro/Shiki renders those markers, so
// normalize generated HTML before deploy.
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDir, "..");
const distRoot = path.join(siteRoot, "dist");
let changedFiles = 0;
let hiddenLines = 0;
let remainingHiddenLines = 0;

for (const filePath of htmlFiles(distRoot)) {
  const html = readFileSync(filePath, "utf8");
  const normalized = normalizeRustdocHtml(html);
  remainingHiddenLines += countRustdocHiddenLines(normalized);

  if (normalized !== html) {
    changedFiles += 1;
    writeFileSync(filePath, normalized);
  }
}

if (remainingHiddenLines > 0) {
  throw new Error(`Generated HTML still contains ${remainingHiddenLines} rustdoc hidden line(s).`);
}

console.log(
  `Normalized ${hiddenLines} rustdoc hidden line(s) in ${changedFiles} generated HTML file(s).`,
);

function* htmlFiles(dir) {
  for (const name of readdirSync(dir)) {
    const filePath = path.join(dir, name);
    const stats = statSync(filePath);

    if (stats.isDirectory()) {
      yield* htmlFiles(filePath);
      continue;
    }

    if (name.endsWith(".html")) {
      yield filePath;
    }
  }
}

function normalizeRustdocHtml(html) {
  return html.replace(
    /(<pre\b[^>]*\bdata-language="(?:rust|rs)"[^>]*><code>)([\s\S]*?)(<\/code><\/pre>)/g,
    (_, open, code, close) => `${open}${normalizeRustdocCode(code)}${close}`,
  );
}

function normalizeRustdocCode(code) {
  const lines = code.split(/\n(?=<span class="line">)/);
  const kept = [];

  for (const line of lines) {
    const text = visiblePrefix(line);
    const escaped = /^(\s*)##/.exec(text);
    if (escaped) {
      kept.push(removeHashAfterIndent(line, escaped[1].length));
      continue;
    }

    if (/^\s*#(?:\s|$)/.test(text)) {
      hiddenLines += 1;
      continue;
    }

    kept.push(line);
  }

  return kept.join("\n");
}

function countRustdocHiddenLines(html) {
  let count = 0;
  html.replace(
    /<pre\b[^>]*\bdata-language="(?:rust|rs)"[^>]*><code>([\s\S]*?)<\/code><\/pre>/g,
    (_, code) => {
      for (const line of code.split(/\n(?=<span class="line">)/)) {
        const text = visiblePrefix(line);
        if (/^\s*#(?:\s|$)/.test(text)) {
          count += 1;
        }
      }
      return "";
    },
  );
  return count;
}

function visiblePrefix(line) {
  let prefix = "";
  for (const match of line.matchAll(/>([^<]*)/g)) {
    prefix += match[1];
    if (/^\s*##/.test(prefix) || /^\s*#(?:\s|$)/.test(prefix)) {
      return prefix;
    }
    if (prefix.trimStart().length > 0) {
      return prefix;
    }
  }

  return prefix;
}

function removeHashAfterIndent(html, indentLength) {
  let remaining = indentLength;
  let removed = false;

  return html.replace(/(>)([^<]*)/g, (match, close, text) => {
    if (removed) {
      return match;
    }

    if (remaining >= text.length) {
      remaining -= text.length;
      return match;
    }

    removed = true;
    return `${close}${text.slice(0, remaining)}${text.slice(remaining + 1)}`;
  });
}

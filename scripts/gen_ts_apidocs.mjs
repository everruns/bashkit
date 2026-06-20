#!/usr/bin/env node
// Generate the TypeScript API reference markdown for bashkit.sh.
//
// Decision: mirror the Python generator (scripts/gen_python_apidocs.py). We
// emit plain markdown and let the Astro DocsLayout render it on-brand, instead
// of shipping TypeDoc's stock HTML theme. Source of truth is the package's own
// TypeScript wrappers (wrapper.ts + subpath modules), parsed by TypeDoc to JSON
// and rendered here for full control over headings, anchors, and links (no
// local .md links — verify-public-links forbids them).
//
// Prerequisite: the .ts wrappers import types from the napi-generated, gitignored
// `index.d.ts`, so `napi build` MUST have run in crates/bashkit-js first. That's
// a Rust compile, so this can't run in the node-only site.yml — run it in a
// Rust-capable release job, then commit the output.
//
// Usage: node scripts/gen_ts_apidocs.mjs   (or: just apidocs-ts)

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const TYPEDOC_VERSION = "0.28.1";
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const jsDir = path.join(repoRoot, "crates", "bashkit-js");
const outPath = path.join(repoRoot, "site", "src", "content", "apidocs", "typescript.md");

const ENTRY_POINTS = ["wrapper.ts", "langchain.ts", "anthropic.ts", "openai.ts", "ai.ts"];

// Friendly module titles + import specifiers, in render order.
const MODULES = {
  wrapper: { title: null, specifier: "@everruns/bashkit", core: true },
  langchain: { title: "@everruns/bashkit/langchain", specifier: "@everruns/bashkit/langchain" },
  anthropic: { title: "@everruns/bashkit/anthropic", specifier: "@everruns/bashkit/anthropic" },
  openai: { title: "@everruns/bashkit/openai", specifier: "@everruns/bashkit/openai" },
  ai: { title: "@everruns/bashkit/ai", specifier: "@everruns/bashkit/ai" },
};

// TypeDoc ReflectionKind values we care about.
const Kind = {
  Module: 2,
  Variable: 32,
  Function: 64,
  Class: 128,
  Interface: 256,
  Constructor: 512,
  Property: 1024,
  Method: 2048,
  Accessor: 262144,
  TypeAlias: 2097152,
};

// Preferred ordering for the core module's classes.
const CLASS_ORDER = ["Bash", "BashTool", "ScriptedTool", "FileSystem", "BashError"];

function runTypedoc() {
  const tmp = mkdtempSync(path.join(tmpdir(), "bashkit-tsdoc-"));
  const jsonPath = path.join(tmp, "api.json");
  execFileSync(
    "npx",
    [
      "--yes",
      `typedoc@${TYPEDOC_VERSION}`,
      "--json",
      jsonPath,
      "--tsconfig",
      path.join(jsDir, "tsconfig.json"),
      "--excludePrivate",
      "--excludeProtected",
      "--excludeExternals",
      "--readme",
      "none",
      "--entryPoints",
      ...ENTRY_POINTS.map((e) => path.join(jsDir, e)),
    ],
    { cwd: jsDir, stdio: ["ignore", "inherit", "inherit"] },
  );
  return JSON.parse(readFileSync(jsonPath, "utf8"));
}

// ---- type stringification --------------------------------------------------

function typeToString(t) {
  if (!t) return "";
  switch (t.type) {
    case "intrinsic":
      return t.name;
    case "literal":
      return typeof t.value === "string" ? `"${t.value}"` : String(t.value);
    case "reference": {
      const args = t.typeArguments?.length
        ? `<${t.typeArguments.map(typeToString).join(", ")}>`
        : "";
      return `${t.name}${args}`;
    }
    case "array":
      return `${wrapIfComplex(t.elementType)}[]`;
    case "union":
      return t.types.map(typeToString).join(" | ");
    case "intersection":
      return t.types.map(typeToString).join(" & ");
    case "tuple":
      return `[${(t.elements ?? []).map(typeToString).join(", ")}]`;
    case "named-tuple-member":
      return `${t.name}${t.isOptional ? "?" : ""}: ${typeToString(t.element)}`;
    case "optional":
      return `${typeToString(t.elementType)}?`;
    case "rest":
      return `...${typeToString(t.elementType)}`;
    case "indexedAccess":
      return `${typeToString(t.objectType)}[${typeToString(t.indexType)}]`;
    case "typeOperator":
      return `${t.operator} ${typeToString(t.target)}`;
    case "query":
      return `typeof ${typeToString(t.queryType)}`;
    case "predicate":
      return `${t.name} is ${typeToString(t.targetType)}`;
    case "templateLiteral":
      return "`template`";
    case "reflection":
      return reflectionTypeToString(t.declaration);
    case "mapped":
    case "conditional":
      return t.name ?? "object";
    case "unknown":
      return t.name ?? "unknown";
    default:
      return t.name ?? "unknown";
  }
}

function wrapIfComplex(t) {
  const s = typeToString(t);
  return /[ |&]/.test(s) ? `(${s})` : s;
}

function reflectionTypeToString(decl) {
  if (!decl) return "object";
  if (decl.signatures?.length) {
    const sig = decl.signatures[0];
    const params = (sig.parameters ?? [])
      .map((p) => `${p.name}: ${typeToString(p.type)}`)
      .join(", ");
    return `(${params}) => ${typeToString(sig.type)}`;
  }
  if (decl.children?.length) {
    const props = decl.children
      .map((c) => `${c.name}${c.flags?.isOptional ? "?" : ""}: ${typeToString(c.type)}`)
      .join("; ");
    return `{ ${props} }`;
  }
  return "object";
}

// ---- comment rendering -----------------------------------------------------

function partsToText(parts) {
  if (!parts) return "";
  return parts
    .map((p) => {
      if (p.kind === "inline-tag" && (p.tag === "@link" || p.tag === "@linkcode")) {
        return `\`${p.tsLinkText || p.text}\``;
      }
      return p.text;
    })
    .join("");
}

function commentToMarkdown(comment) {
  if (!comment) return { summary: "", examples: [], remarks: "" };
  const summary = partsToText(comment.summary).trim();
  const examples = [];
  let remarks = "";
  for (const tag of comment.blockTags ?? []) {
    if (tag.tag === "@example") {
      examples.push(partsToText(tag.content).trim());
    } else if (tag.tag === "@remarks") {
      remarks = partsToText(tag.content).trim();
    }
  }
  return { summary, examples, remarks };
}

function renderComment(comment, lines) {
  const { summary, examples, remarks } = commentToMarkdown(comment);
  if (summary) {
    lines.push(summary, "");
  }
  if (remarks) {
    lines.push(remarks, "");
  }
  for (const ex of examples) {
    // @example content already contains fenced code in our JSDoc.
    lines.push(ex, "");
  }
}

// ---- member rendering ------------------------------------------------------

// Lowercase the leading character so a class name reads as an instance
// receiver: `Bash` -> `bash`, `BashTool` -> `bashTool`.
function instanceName(className) {
  return className.charAt(0).toLowerCase() + className.slice(1);
}

// A field bullet: `- **name** — `type`` with the description as an indented
// continuation paragraph. Multi-paragraph summaries must be indented under the
// bullet, otherwise their blank lines terminate the list and the trailing
// paragraphs render dedented (one `<ul><li>` per field + orphaned `<p>`s).
function fieldEntry(label, typeStr, summary) {
  const head = `- **\`${label}\`** — \`${typeStr}\``;
  if (!summary) return [head];
  const body = summary
    .split("\n")
    .map((l) => (l.length ? `  ${l}` : ""))
    .join("\n");
  return [head, "", body];
}

function signatureString(name, sig) {
  const params = (sig.parameters ?? [])
    .map((p) => {
      const rest = p.flags?.isRest ? "..." : "";
      const opt = p.flags?.isOptional ? "?" : "";
      return `${rest}${p.name}${opt}: ${typeToString(p.type)}`;
    })
    .join(", ");
  return `${name}(${params}): ${typeToString(sig.type)}`;
}

function renderCallable(name, refl, level, heading) {
  const lines = [`${"#".repeat(level)} ${heading}`, ""];
  const sig = refl.signatures?.[0];
  if (sig) {
    lines.push("```typescript", signatureString(name, sig), "```", "");
    renderComment(sig.comment ?? refl.comment, lines);
  } else {
    renderComment(refl.comment, lines);
  }
  return lines;
}

function renderClass(refl) {
  const lines = [`## ${refl.name}`, ""];
  renderComment(refl.comment, lines);

  const children = refl.children ?? [];
  const ctor = children.find((c) => c.kind === Kind.Constructor);
  const methods = children.filter((c) => c.kind === Kind.Method);
  const accessors = children.filter((c) => c.kind === Kind.Accessor);
  const props = children.filter((c) => c.kind === Kind.Property);

  if (props.length) {
    lines.push("### Properties", "");
    for (const p of props) {
      const { summary } = commentToMarkdown(p.comment);
      lines.push(...fieldEntry(p.name, typeToString(p.type), summary));
    }
    lines.push("");
  }

  if (ctor?.signatures?.length) {
    lines.push(...renderCallable(`new ${refl.name}`, ctor, 3, "Constructor"));
  }
  for (const a of accessors) {
    const getter = a.getSignature;
    if (getter) {
      lines.push(`### \`${a.name}\``, "");
      lines.push("```typescript", `${a.name}: ${typeToString(getter.type)}`, "```", "");
      renderComment(getter.comment ?? a.comment, lines);
    }
  }
  for (const m of methods) {
    // Static methods are class-level (`Bash.create`); instance methods read as
    // a call on an instance (`bash.addBuiltin`), not a static `Bash.addBuiltin`.
    const receiver = m.flags?.isStatic ? refl.name : instanceName(refl.name);
    lines.push(...renderCallable(`${receiver}.${m.name}`, m, 3, `\`${m.name}\``));
  }
  return lines;
}

function renderInterface(refl) {
  const lines = [`## ${refl.name}`, ""];
  renderComment(refl.comment, lines);
  const children = refl.children ?? [];
  const props = children.filter((c) => c.kind === Kind.Property);
  const methods = children.filter((c) => c.kind === Kind.Method);
  if (props.length) {
    lines.push("### Fields", "");
    for (const p of props) {
      const opt = p.flags?.isOptional ? "?" : "";
      const { summary } = commentToMarkdown(p.comment);
      lines.push(...fieldEntry(`${p.name}${opt}`, typeToString(p.type), summary));
    }
    lines.push("");
  }
  for (const m of methods) {
    lines.push(...renderCallable(m.name, m, 3, `\`${m.name}\``));
  }
  return lines;
}

function renderTypeAlias(refl) {
  const lines = [`## ${refl.name}`, ""];
  renderComment(refl.comment, lines);
  lines.push("```typescript", `type ${refl.name} = ${typeToString(refl.type)}`, "```", "");
  return lines;
}

function renderModuleMembers(mod, out) {
  const children = mod.children ?? [];
  const byKind = (k) => children.filter((c) => c.kind === k);

  const classes = byKind(Kind.Class).sort((a, b) => {
    const ia = CLASS_ORDER.indexOf(a.name);
    const ib = CLASS_ORDER.indexOf(b.name);
    return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib);
  });
  for (const c of classes) out.push(...renderClass(c));
  for (const f of byKind(Kind.Function)) {
    out.push(...renderCallable(f.name, f, 2, `${f.name}()`));
  }
  for (const i of byKind(Kind.Interface)) out.push(...renderInterface(i));
  for (const t of byKind(Kind.TypeAlias)) out.push(...renderTypeAlias(t));
}

// ---- main ------------------------------------------------------------------

function main() {
  const project = runTypedoc();
  // With multiple entry points, project.children are the modules. With one it
  // flattens — normalize to a module map keyed by entry basename.
  const modules = new Map();
  if ((project.children ?? []).some((c) => c.kind === Kind.Module)) {
    for (const m of project.children) modules.set(m.name, m);
  } else {
    modules.set("wrapper", project);
  }

  const out = [];
  out.push("# TypeScript API reference", "");
  out.push(
    "Auto-generated reference for the " +
      "[`@everruns/bashkit`](https://www.npmjs.com/package/@everruns/bashkit) " +
      "npm package. Reflects the latest published release.",
    "",
  );
  out.push(
    "> Install with `npm install @everruns/bashkit`. See the " +
      "[Embedding guide](/docs/embedding/) and " +
      "[LLM tools guide](/docs/llm-tools/) for task-oriented walkthroughs.",
    "",
  );

  // Core module first.
  const core = modules.get("wrapper");
  if (core) renderModuleMembers(core, out);

  // Integration subpath modules.
  const integ = [];
  for (const [key, meta] of Object.entries(MODULES)) {
    if (meta.core) continue;
    const mod = modules.get(key);
    if (!mod || !(mod.children ?? []).length) continue;
    integ.push(`## \`${meta.title}\``, "");
    renderComment(mod.comment, integ);
    const sub = [];
    renderModuleMembers(mod, sub);
    // Demote H2s from members to H3s under the module section.
    for (const line of sub) {
      integ.push(line.startsWith("## ") ? `###${line.slice(2)}` : line);
    }
  }
  if (integ.length) {
    out.push("---", "", "# Framework integrations", "", ...integ);
  }

  mkdirSync(path.dirname(outPath), { recursive: true });
  const text = out.join("\n").replace(/\n{3,}/g, "\n\n").trimEnd() + "\n";
  writeFileSync(outPath, text, "utf8");
  console.log(`wrote ${path.relative(repoRoot, outPath)} (${text.length} bytes)`);
}

main();

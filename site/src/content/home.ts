// Decision: homepage HTML and Markdown are generated from the same content
// tables so agent-facing navigation and product claims stay in sync.
// Decision: the builtin count comes from the generated inventory
// (knowledge/status/builtins.json) so marketing copy can't drift from the code.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const inventory = JSON.parse(
  readFileSync(resolve(process.cwd(), "../knowledge/status/builtins.json"), "utf8"),
) as { builtins: { name: string }[] };

export const builtinCount = inventory.builtins.length;

// Decision: the homepage eval table is derived from the generated performance
// timeline (site/scripts/build-performance-data.mjs, fed by
// crates/bashkit-eval/results), not hand-copied, so it can't lag the README.
// It shows every model's latest full run from the newest eval lineup (runs
// within LINEUP_WINDOW_DAYS of the newest one), best score first.
type EvalRun = {
  kind: string;
  model: string;
  timestamp: string | null;
  date: string;
  tasks: number;
  passed: number;
  scorePct: number;
};
const timeline = JSON.parse(
  readFileSync(resolve(process.cwd(), "src/data/performance-timeline.json"), "utf8"),
) as { evalRuns: EvalRun[] };

const LINEUP_WINDOW_DAYS = 7;
const DAY_MS = 24 * 60 * 60 * 1000;

export function evalModelName(model: string): string {
  const claude = model.match(/^claude-([a-z]+)-(\d+)-(\d+)/);
  if (claude) {
    const [, family, major, minor] = claude;
    return `Claude ${family[0].toUpperCase()}${family.slice(1)} ${major}.${minor}`;
  }
  const gpt = model.match(/^gpt-([\d.]+)(?:-(.+))?$/);
  if (gpt) {
    const [, version, variant] = gpt;
    if (!variant) return `GPT-${version}`;
    if (variant === "codex") return `GPT-${version}-Codex`;
    return `GPT-${version} ${variant[0].toUpperCase()}${variant.slice(1)}`;
  }
  const gemini = model.match(/^gemini-([\d.]+)-(.+)$/);
  if (gemini) {
    const [, version, variant] = gemini;
    const words = variant.split("-").map((w) => w[0].toUpperCase() + w.slice(1));
    return `Gemini ${version} ${words.join(" ")}`;
  }
  const kimi = model.match(/^kimi-k([\d.]+)$/);
  if (kimi) return `Kimi K${kimi[1]}`;
  const muse = model.match(/^muse-([a-z]+)-([\d.]+)(?:-(.+))?$/);
  if (muse) {
    const [, line, version, variant] = muse;
    const cap = (w: string) => w[0].toUpperCase() + w.slice(1);
    return `Muse ${cap(line)} ${version}${variant ? ` ${cap(variant)}` : ""}`;
  }
  return model;
}

function latestEvalLineup(runs: EvalRun[]) {
  const full = runs.filter((run) => run.kind === "llm-eval" && run.tasks >= 50 && run.timestamp);
  const newest = Math.max(...full.map((run) => new Date(run.timestamp!).getTime()));
  const byModel = new Map<string, EvalRun>();
  for (const run of full) {
    if (newest - new Date(run.timestamp!).getTime() > LINEUP_WINDOW_DAYS * DAY_MS) continue;
    const prev = byModel.get(run.model);
    if (!prev || new Date(run.timestamp!) > new Date(prev.timestamp!)) byModel.set(run.model, run);
  }
  return [...byModel.values()].sort((a, b) => b.scorePct - a.scorePct || b.passed - a.passed);
}

const evalLineup = latestEvalLineup(timeline.evalRuns);

export const homeHero = {
  eyebrow: "Virtual bash for AI agents",
  title: "An awesomely fast virtual bash sandbox. Written in Rust.",
  description:
    `Bashkit runs untrusted shell scripts from AI agents without spawning a single OS process. ${builtinCount} reimplemented commands, substantial POSIX shell language coverage, a virtual filesystem, resource limits, and tool interfaces for agent frameworks \u2014 all in-memory, all sandboxed.`,
};

export const homeNavigation = [
  {
    label: "Docs",
    href: "/docs/",
    detail: "User-facing guides for the CLI, security model, runtimes, and extension APIs.",
  },
  {
    label: "Builtins",
    href: "/builtins",
    detail: `Browse the ${builtinCount}-command sandbox surface.`,
  },
  {
    label: "GitHub repository",
    href: "https://github.com/everruns/bashkit",
    detail: "Source, issues, examples, and releases.",
  },
  {
    label: "Bashkit agent skill",
    href: "https://bashkit.sh/.well-known/agent-skills/index.json",
    detail: "Machine-readable skill index for coding agents.",
  },
  {
    label: "Everruns",
    href: "https://everruns.com",
    detail: "The product ecosystem around Bashkit.",
  },
];

export const evalSnapshot = {
  date: evalLineup.map((run) => run.date).sort().at(-1) ?? "unknown",
  href: "https://github.com/everruns/bashkit/blob/main/crates/bashkit-eval/README.md",
};

export const benchesHref = "/benches";

// Human-readable skill source (SKILL.md + references) for "View skill contents".
export const skillRepoUrl =
  "https://github.com/everruns/bashkit/tree/main/skills/bashkit";

// Ready-to-paste prompts for driving a coding agent that has the skill installed.
export const commonPrompts = [
  "Using bashkit, add a sandboxed bash tool to my agent.",
  "Embed bashkit in my Rust service to run untrusted shell scripts in-memory.",
  "Run this bash script in bashkit and return stdout, stderr, and the exit code.",
  "Add a custom builtin to bashkit that calls my internal HTTP API.",
];

export const heroStats = [
  { label: "Built-in commands", value: String(builtinCount), href: "/builtins" },
  {
    label: "Threats mitigated",
    value: "250+",
    href: "https://github.com/everruns/bashkit/blob/main/knowledge/security/threat-model.md",
    external: true,
  },
  {
    label: `${evalModelName(evalLineup[0]?.model ?? "")} eval`,
    value: `${Math.round(evalLineup[0]?.scorePct ?? 0)}%`,
    href: evalSnapshot.href,
    external: true,
  },
];

export const quickStarts = [
  { label: "Agents", href: "#agent-development" },
  { label: "Rust", href: "#quickstart-rust" },
  { label: "Python", href: "#quickstart-python" },
  { label: "TypeScript", href: "#quickstart-typescript" },
];

export const heroQuickLinks = [
  {
    title: "Rust docs",
    detail: "docs.rs reference",
    href: "https://docs.rs/bashkit",
  },
  {
    title: "Examples",
    detail: "Rust, Python, TS",
    href: "https://github.com/everruns/bashkit/tree/main/examples",
  },
];

export const builtinPreview = [
  "grep",
  "sed",
  "awk",
  "jq",
  "curl",
  "find",
  "xargs",
  "tar",
  "git",
  "ssh",
  "python",
  "typescript",
];

export const signals = [
  {
    title: "No process spawning",
    detail:
      "Every command is reimplemented in Rust. No fork, no exec, no shell escape.",
  },
  {
    title: "Virtual filesystem",
    detail:
      "InMemoryFs, OverlayFs, MountableFs. Host access only when you explicitly mount it.",
  },
  {
    title: "Resource limits",
    detail:
      "Caps on commands, loops, output, input, and filesystem size. DoS-resistant by construction.",
  },
];

export const agentSteps = [
  {
    title: "Install the skill",
    detail: "Give your coding agent Bashkit-specific usage notes and examples.",
    command: "npx skills add everruns/bashkit",
  },
  {
    title: "Ask agent to add it",
    detail: "Prompt your coding agent to wire Bashkit into the host project.",
    command: 'Using bashkit, add support for a bash tool',
  },
  {
    title: "Enjoy :)",
    detail: "Use the new bash tool in your agent workflow.",
  },
];

export const surfaces = [
  {
    title: "POSIX-compliant interpreter",
    detail:
      "Substantial IEEE 1003.1-2024 Shell Command Language coverage, plus bash extensions: arrays, [[ ]], brace expansion, extended globs, coprocesses, traps.",
  },
  {
    title: `${builtinCount} reimplemented commands`,
    detail:
      "grep, sed, awk, jq, curl, tar, find, xargs, and 150+ more \u2014 pure Rust, no shelling out.",
  },
  {
    title: "LLM tool contract",
    detail:
      "BashTool with discovery metadata, streaming output, and system prompts. Plug into any agent framework.",
  },
  {
    title: "Interactive shell",
    detail:
      "Run bashkit with no args for a local REPL with line editing and multiline input.",
  },
  {
    title: "Snapshotting",
    detail:
      "Serialize shell state and VFS contents to bytes. Checkpoint any workload, resume anywhere.",
  },
  {
    title: "Scripted tool orchestration",
    detail:
      "Compose ToolDef + callback pairs into a ScriptedTool driven by a bash script.",
  },
];

export const heroSnippet = `use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::new();
    let out = bash.exec("printf 'ready\\n'").await?;
    print!("{}", out.stdout);
    Ok(())
}`;

export const languages = [
  {
    slug: "rust",
    eyebrow: "Rust",
    title: "The core crate",
    install: "cargo add bashkit",
    lang: "rust" as const,
    code: `use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::new();
    let out = bash.exec("echo hello world").await?;
    println!("{}", out.stdout);
    Ok(())
}`,
  },
  {
    slug: "python",
    eyebrow: "Python",
    title: "PyO3 wheel with direct Bash API",
    install: "pip install bashkit",
    lang: "python" as const,
    code: `from bashkit import Bash

bash = Bash()
result = bash.execute_sync("echo 'Hello, World!'")
print(result.stdout)

bash.execute_sync("export APP_ENV=dev")
print(bash.execute_sync("echo $APP_ENV").stdout)`,
  },
  {
    slug: "typescript",
    eyebrow: "TypeScript",
    title: "NAPI-RS runtime for Node, Bun, Deno",
    install: "npm i @everruns/bashkit",
    lang: "ts" as const,
    code: `import { Bash } from "@everruns/bashkit";

const bash = new Bash();
const result = bash.executeSync('echo "Hello, World!"');
console.log(result.stdout);

bash.executeSync("X=42");
console.log(bash.executeSync("echo $X").stdout);`,
  },
];

export const defense = [
  {
    title: "No process spawning",
    detail:
      `${builtinCount} commands reimplemented in Rust \u2014 no fork, exec, or shell escape.`,
  },
  {
    title: "Virtual filesystem",
    detail:
      "Scripts see an in-memory FS by default. No host access unless mounted.",
  },
  {
    title: "Network allowlist",
    detail: "HTTP is denied by default. Each domain must be explicitly allowed.",
  },
  {
    title: "Resource limits",
    detail:
      "Caps on commands (10K), loops (100K), function depth, output (10MB), input (10MB).",
  },
  {
    title: "Parser limits",
    detail:
      "Timeout, fuel budget, AST depth \u2014 pathological input can't hang the interpreter.",
  },
  {
    title: "Panic recovery",
    detail:
      "Every builtin is wrapped in catch_unwind. A panic in one command can't crash the host.",
  },
];

export const evals = evalLineup.map((run) => ({
  model: evalModelName(run.model),
  score: `${Math.round(run.scorePct)}%`,
  passed: `${run.passed}/${run.tasks}`,
}));

export const resources = [
  {
    title: "Rust API",
    detail: "Core crate docs, builder options, limits, and shell semantics.",
    href: "https://docs.rs/bashkit",
    cta: "docs.rs",
  },
  {
    title: "Python",
    detail: "PyO3 package docs for direct Bash usage, snapshots, and builtins.",
    href: "https://github.com/everruns/bashkit/blob/main/crates/bashkit-python/README.md",
    cta: "Python docs",
  },
  {
    title: "TypeScript",
    detail: "Node, Bun, and Deno runtime docs for the NAPI bindings.",
    href: "https://github.com/everruns/bashkit/blob/main/crates/bashkit-js/README.md",
    cta: "TS docs",
  },
  {
    title: "Threat model",
    detail: "268 documented threat cases across parser, VFS, network, and runtimes.",
    href: "https://github.com/everruns/bashkit/blob/main/knowledge/security/threat-model.md",
    cta: "Security spec",
  },
  {
    title: "Benches history",
    detail: "Interactive trends across benchmarks, criterion benches, and evals.",
    href: benchesHref,
    cta: "Benches",
  },
  {
    title: "CLI reference",
    detail: "One-shot commands, script execution, and interactive shell usage.",
    href: "https://github.com/everruns/bashkit/blob/main/docs/cli.md",
    cta: "CLI docs",
  },
  {
    title: "Examples",
    detail: "Reference programs for Rust, Python, JavaScript, and tool flows.",
    href: "https://github.com/everruns/bashkit/tree/main/examples",
    cta: "Browse examples",
  },
];

export const apiSnippet = `use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::new();
    bash.exec("mkdir -p /tmp/data").await?;
    bash.exec("echo 'hello' > /tmp/data/out.txt").await?;

    let r = bash.exec("cat /tmp/data/out.txt | tr a-z A-Z").await?;
    print!("{}", r.stdout); // HELLO
    Ok(())
}`;

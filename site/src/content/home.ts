// Decision: homepage HTML and Markdown are generated from the same content
// tables so agent-facing navigation and product claims stay in sync.
// Decision: the builtin count comes from the generated inventory
// (specs/status/builtins.json) so marketing copy can't drift from the code.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const inventory = JSON.parse(
  readFileSync(resolve(process.cwd(), "../specs/status/builtins.json"), "utf8"),
) as { builtins: { name: string }[] };

export const builtinCount = inventory.builtins.length;

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
  date: "2026-02-28",
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
    href: "https://github.com/everruns/bashkit/blob/main/specs/threat-model.md",
    external: true,
  },
  {
    label: "Haiku 4.5 eval",
    value: "97%",
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

export const evals = [
  { model: "Claude Haiku 4.5", score: "97%", passed: "54/58" },
  { model: "Claude Sonnet 4.6", score: "93%", passed: "48/58" },
  { model: "Claude Opus 4.6", score: "91%", passed: "50/58" },
  { model: "GPT-5.3-Codex", score: "91%", passed: "51/58" },
  { model: "GPT-5.2", score: "77%", passed: "41/58" },
];

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
    href: "https://github.com/everruns/bashkit/blob/main/specs/threat-model.md",
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

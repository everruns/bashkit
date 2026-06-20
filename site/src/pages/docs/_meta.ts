// Hand-curated metadata for each doc so the /docs index can group and
// summarise pages without requiring frontmatter in the canonical markdown.
// Order here drives navigation order (index cards, prev/next).

export type DocMeta = {
  slug: string;
  title: string;
  summary: string;
  section: string;
  seoTitle?: string;
  seoDescription?: string;
  collection?: "docs" | "rustdocs";
  sourceId?: string;
  editPath?: string;
};

// Single source of truth for section display order, shared by the /docs index
// (index.astro) and its Markdown twin (docs.md.ts). DOC_META is ordered to match
// so the per-page prev/next flow follows the same grouping.
export const SECTION_ORDER = [
  "Getting started",
  "LLM tools",
  "Concepts",
  "Networking",
  "Runtimes",
  "Reference",
  "Extending",
  "Operations",
];

export const DOC_META: DocMeta[] = [
  {
    slug: "cli",
    title: "CLI",
    summary: "Run scripts with bashkit-cli: flags, exit codes, opt-in runtimes.",
    seoTitle: "bashkit-cli docs: run sandboxed shell scripts",
    seoDescription:
      "Learn bashkit-cli modes, install options, limits, host filesystem mounts, interactive shell usage, and sandboxed script examples.",
    section: "Getting started",
  },
  {
    slug: "embedding",
    title: "Embedding",
    summary: "Run Bashkit as a library in Rust, Python, or TypeScript.",
    seoTitle: "Embed Bashkit as a library in Rust, Python, TypeScript",
    seoDescription:
      "Embed the Bashkit sandbox as a library in Rust, Python, or TypeScript: run scripts in-process, persist shell state, set resource limits, and allowlist HTTP.",
    section: "Getting started",
    editPath: "docs/embedding.md",
  },
  {
    slug: "llm-tools",
    title: "LLM tools",
    summary: "Expose Bashkit as a sandboxed tool for agent frameworks.",
    seoTitle: "Use Bashkit as an LLM tool for agent frameworks",
    seoDescription:
      "Expose Bashkit as an LLM tool with BashTool: discovery metadata, system prompts, streaming output, and sandboxed execution for Rust, Python, and JS agents.",
    section: "LLM tools",
    editPath: "docs/llm-tools.md",
  },
  {
    slug: "scripted-tools",
    title: "Scripted tool orchestration",
    summary: "Compose many tools into one bash-scriptable tool the LLM calls once.",
    seoTitle: "Bashkit scripted tool orchestration for LLM agents",
    seoDescription:
      "Compose ToolDef and callback pairs into one ScriptedTool so an LLM orchestrates many tools in a single bash script with pipes, loops, and jq.",
    section: "LLM tools",
    editPath: "docs/scripted-tools.md",
  },
  {
    slug: "filesystem",
    title: "Virtual filesystem",
    summary: "The in-memory VFS, its layering stack, and host-mount opt-ins.",
    seoTitle: "Bashkit virtual filesystem and sandbox layering",
    seoDescription:
      "Understand Bashkit's in-memory virtual filesystem: the FsBackend and FileSystem traits, layering stack, InMemoryFs, RealFs, device files, and host mounts.",
    section: "Concepts",
    editPath: "docs/filesystem.md",
  },
  {
    slug: "security",
    title: "Security",
    summary: "Sandbox boundaries, threat model, and what scripts cannot do.",
    seoTitle: "Bashkit security model and sandbox boundaries",
    seoDescription:
      "Review Bashkit's virtual filesystem, resource limits, network allowlists, POSIX security deviations, and threat model guidance.",
    section: "Concepts",
  },
  {
    slug: "networking",
    title: "Networking & HTTP",
    summary: "Default-deny HTTP, the network allowlist, and SSRF protection.",
    seoTitle: "Bashkit networking, HTTP allowlist, and SSRF protection",
    seoDescription:
      "Configure Bashkit outbound HTTP with curl, wget, and http: the default-deny NetworkAllowlist, pattern matching, and private-IP and SSRF blocking.",
    section: "Networking",
    editPath: "docs/networking.md",
  },
  {
    slug: "credential-injection",
    title: "Credential injection",
    summary: "Inject outbound HTTP credentials without exposing secrets to scripts.",
    seoTitle: "Bashkit credential injection for sandboxed HTTP",
    seoDescription:
      "Inject bearer tokens and headers into Bashkit outbound HTTP requests without exposing real secrets to sandboxed shell scripts.",
    section: "Networking",
    collection: "rustdocs",
    editPath: "crates/bashkit/docs/credential-injection.md",
  },
  {
    slug: "request-signing",
    title: "Request signing",
    summary: "Transparent Ed25519 bot identity (RFC 9421) on every HTTP request.",
    seoTitle: "Bashkit request signing: cryptographic identity for AI agents",
    seoDescription:
      "Sign every outbound Bashkit HTTP request with Ed25519 per RFC 9421 (web-bot-auth): transparent bot identity and verifiable agent traffic.",
    section: "Networking",
    editPath: "docs/request-signing.md",
  },
  {
    slug: "python",
    title: "Python builtin",
    summary: "Embedded Monty Python runtime, VFS bridging, limits, and caveats.",
    seoTitle: "Bashkit Python builtin with Monty runtime",
    seoDescription:
      "Run embedded Python in Bashkit with Monty, virtual filesystem bridging, pipelines, command substitution, resource limits, and safety caveats.",
    section: "Runtimes",
    collection: "rustdocs",
    editPath: "crates/bashkit/docs/python.md",
  },
  {
    slug: "builtin_typescript",
    title: "TypeScript builtin",
    summary: "Embedded ZapCode TypeScript runtime shared with bash in-memory.",
    seoTitle: "Bashkit TypeScript builtin with ZapCode runtime",
    seoDescription:
      "Run TypeScript inside Bashkit with ZapCode, VFS file sharing, pipelines, resource limits, and Node.js-compatible command aliases.",
    section: "Runtimes",
  },
  {
    slug: "sqlite",
    title: "SQLite builtin",
    summary: "Embedded Turso SQLite runtime, backends, output modes, and limits.",
    seoTitle: "Bashkit SQLite builtin with Turso",
    seoDescription:
      "Use Bashkit's embedded SQLite builtin with Turso, VFS-backed databases, output modes, dot-commands, resource limits, and security notes.",
    section: "Runtimes",
    collection: "rustdocs",
    editPath: "crates/bashkit/docs/sqlite.md",
  },
  {
    slug: "ssh",
    title: "SSH support",
    summary: "Sandboxed ssh, scp, and sftp builtins with host allowlists.",
    seoTitle: "Bashkit SSH, SCP, and SFTP builtin support",
    seoDescription:
      "Configure Bashkit ssh, scp, and sftp builtins with host allowlists, VFS-only keys, remote command execution, and transfer limits.",
    section: "Runtimes",
    collection: "rustdocs",
    editPath: "crates/bashkit/docs/ssh.md",
  },
  {
    slug: "git",
    title: "Git",
    summary: "Sandboxed git on the virtual filesystem with a configurable identity.",
    seoTitle: "Bashkit sandboxed git on the virtual filesystem",
    seoDescription:
      "Run git inside Bashkit on the virtual filesystem: init, add, commit, branch, log, and allowlisted remotes with a configurable identity and no host access.",
    section: "Runtimes",
    editPath: "docs/git.md",
  },
  {
    slug: "compatibility",
    title: "Compatibility",
    summary: "Bash and builtin feature coverage, security exclusions, and known gaps.",
    seoTitle: "Bashkit compatibility scorecard for bash and builtins",
    seoDescription:
      "Check Bashkit POSIX shell support, implemented builtins, syntax coverage, expansions, resource limits, and known compatibility gaps.",
    section: "Reference",
    collection: "rustdocs",
    editPath: "crates/bashkit/docs/compatibility.md",
  },
  {
    slug: "jq",
    title: "jq builtin",
    summary: "Supported jq flags, filters, variables, exit codes, and compatibility notes.",
    seoTitle: "Bashkit jq builtin compatibility guide",
    seoDescription:
      "See which jq flags, filters, variables, exit statuses, errors, and compatibility gaps Bashkit supports through its embedded jq builtin.",
    section: "Reference",
    collection: "rustdocs",
    editPath: "crates/bashkit/docs/jq.md",
  },
  {
    slug: "structured-data",
    title: "Structured data",
    summary: "CSV, JSON, YAML, and TOML builtins for everyday data wrangling.",
    seoTitle: "Bashkit CSV, JSON, YAML, and TOML builtins",
    seoDescription:
      "Query and transform structured data in Bashkit with the csv, json, yaml, and tomlq builtins: column selection, filtering, and dotted-path lookups.",
    section: "Reference",
    editPath: "docs/structured-data.md",
  },
  {
    slug: "custom-builtins",
    title: "Custom builtins",
    summary: "Implement Rust commands that run inside the Bashkit shell.",
    seoTitle: "Build custom Bashkit builtins in Rust",
    seoDescription:
      "Create custom Bashkit commands in Rust with Builtin, BuiltinContext, virtual filesystem access, execution extensions, and tested examples.",
    section: "Extending",
    collection: "rustdocs",
    sourceId: "custom_builtins",
    editPath: "crates/bashkit/docs/custom_builtins.md",
  },
  {
    slug: "custom-builtins-js",
    title: "Custom builtins (JavaScript)",
    summary: "Register JS callbacks as persistent bash builtins from Node, Deno, or Bun.",
    seoTitle: "Add custom Bashkit builtins from JavaScript",
    seoDescription:
      "Use customBuiltins and addBuiltin in @everruns/bashkit to register JS callbacks as persistent bash commands with virtual filesystem access and async support.",
    section: "Extending",
    sourceId: "custom_builtins_js",
    editPath: "docs/custom_builtins_js.md",
  },
  {
    slug: "clap-builtins",
    title: "Clap builtins",
    summary: "Use clap parser structs to build typed custom commands.",
    seoTitle: "Build typed Bashkit builtins with clap",
    seoDescription:
      "Use ClapBuiltin and clap Parser derives to add typed Bashkit commands with help output, parse errors, subcommands, and pipeline stdin.",
    section: "Extending",
    collection: "rustdocs",
    editPath: "crates/bashkit/docs/clap-builtins.md",
  },
  {
    slug: "hooks",
    title: "Hooks",
    summary: "Observe, modify, or cancel execution, builtin, lifecycle, and HTTP events.",
    seoTitle: "Bashkit hooks for execution, tools, and HTTP events",
    seoDescription:
      "Use Bashkit hooks to observe, rewrite, or cancel script execution, builtins, shell lifecycle events, and allowlisted HTTP requests.",
    section: "Extending",
    collection: "rustdocs",
    editPath: "crates/bashkit/docs/hooks.md",
  },
  {
    slug: "live-mounts",
    title: "Live mounts",
    summary: "Attach, detach, and hot-swap filesystems on a running interpreter.",
    seoTitle: "Bashkit live mounts for virtual filesystems",
    seoDescription:
      "Attach, detach, and hot-swap Bashkit virtual filesystems on a running interpreter while preserving shell state and mounted data.",
    section: "Extending",
    collection: "rustdocs",
    sourceId: "live_mounts",
    editPath: "crates/bashkit/docs/live_mounts.md",
  },
  {
    slug: "snapshotting",
    title: "Snapshotting",
    summary: "Serialize interpreter state and restore it for checkpoint/resume flows.",
    seoTitle: "Bashkit snapshotting for checkpoint and resume workflows",
    seoDescription:
      "Use Bashkit snapshots to serialize and restore virtual shell state across Rust, Python, and Node.js checkpoint/resume workflows.",
    section: "Operations",
  },
  {
    slug: "logging",
    title: "Logging",
    summary: "Structured tracing setup, log targets, and redaction behavior.",
    seoTitle: "Bashkit structured logging and tracing guide",
    seoDescription:
      "Enable Bashkit structured logging with tracing, configure log targets and levels, and redact sensitive script, URL, and environment data.",
    section: "Operations",
    collection: "rustdocs",
    editPath: "crates/bashkit/docs/logging.md",
  },
];

export const DOC_META_BY_SLUG: Record<string, DocMeta> = Object.fromEntries(
  DOC_META.map((doc) => [doc.slug, doc]),
);

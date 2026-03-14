/**
 * Pi extension: replaces bash, read, write, and edit tools with bashkit virtual implementations.
 *
 * Uses @everruns/bashkit Node.js bindings (NAPI-RS) — no subprocess, no Python.
 * All operations run against bashkit's in-memory virtual filesystem.
 * State (variables, files, cwd) persists across tool calls within a session.
 *
 * read/write/edit use direct VFS APIs (readFile, writeFile, mkdir, exists).
 * bash tool uses executeSync for shell commands.
 * Both share the same Bash instance so VFS and shell state are always in sync.
 *
 * Usage:
 *   cd examples/bashkit-pi && npm install
 *   pi -e examples/bashkit-pi/bashkit-extension.ts
 */

import { createRequire } from "node:module";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname_ext =
	typeof __dirname !== "undefined"
		? __dirname
		: dirname(fileURLToPath(import.meta.url));

// Load bashkit native bindings from the bashkit-js crate (or node_modules)
const require_ext = createRequire(resolve(__dirname_ext, "node_modules") + "/");
const { Bash } = require_ext("@everruns/bashkit");

// Single bashkit instance — state persists across all tool calls
const bash = new Bash({ username: "user", hostname: "pi-sandbox" });

// Resolve relative paths against bashkit home
function resolvePath(userPath: string): string {
	if (userPath.startsWith("/")) return userPath;
	return `/home/user/${userPath}`;
}

// Ensure parent directory exists for a file path
function ensureParentDir(filePath: string): void {
	const dir = filePath.replace(/\/[^/]*$/, "");
	if (dir && dir !== filePath && !bash.exists(dir)) {
		bash.mkdir(dir, true);
	}
}

// System prompt snippet explaining bashkit environment to the LLM
const BASHKIT_SYSTEM_PROMPT = `
## Bashkit Virtual Environment

**IMPORTANT**: You are running inside a **bashkit** sandboxed environment. All tools (bash, read, write, edit) operate on a virtual in-memory filesystem — nothing touches the real host filesystem.

### Your environment identity

- **Your working directory is \`/home/user\`** — this is where you start and where relative paths resolve.
- **Ignore any host paths** from runtime context (e.g. \`/Users/...\`, \`/home/...\`, \`C:\\...\`). Those refer to the machine running the harness, NOT your environment. Never reference, display, or use host paths in your responses.
- **You have no access to any project on the host machine.** If the user asks about files, they mean files in YOUR virtual filesystem at \`/home/user\`. If no files exist yet, say so.
- When the user mentions a "current working directory" or "project", it refers to what's inside your virtual filesystem, not the host.

### Key differences from real bash

- **No network access**: \`curl\` and \`wget\` are simulated but do not make real HTTP requests.
- **No package managers**: \`apt\`, \`pip\`, \`npm\`, \`cargo\` etc. are not available. Do not try to install packages.
- **No interpreters**: \`python\`, \`python3\`, \`perl\`, \`node\`, \`ruby\` are not available. Write bash-native solutions using the available builtins.
- **No \`git\`**: Version control commands are not available.
- **No \`sudo\`**: Everything runs as a regular user. Permission commands (\`chmod\`, \`chown\`) are accepted but have no real OS effect.
- **Virtual filesystem**: All paths (e.g. \`/home/user\`, \`/tmp\`) exist in memory only. Files persist across tool calls within the same session but are gone when the session ends.
- **State persists**: Shell variables, functions, cwd, and files carry over between bash tool calls.

### Available builtins (100+)

**Core I/O**: echo, printf, cat, read
**Text processing**: grep, sed, awk, jq, head, tail, sort, uniq, cut, tr, wc, nl, paste, column, comm, diff, strings, tac, rev
**File operations**: cd, pwd, ls, find, mkdir, mktemp, rm, rmdir, cp, mv, touch, chmod, chown, ln
**File inspection**: file, stat, less, tar, gzip, gunzip, du, df
**Flow control**: test, [, true, false, exit, return, break, continue
**Shell/variables**: export, set, unset, local, shift, source, eval, declare, typeset, readonly, shopt, getopts
**Utilities**: sleep, date, seq, expr, yes, wait, timeout, xargs, tee, watch, basename, dirname, realpath
**Dir stack**: pushd, popd, dirs
**System info**: whoami, hostname, uname, id, env, printenv, history
**Binary/hex**: od, xxd, hexdump, base64
**Signals**: kill

### Best practices

- Use bash builtins for all text processing — they are fast and fully functional.
- Create files with the \`write\` tool for large content; use \`bash\` with echo/cat for quick one-liners.
- Use absolute paths (start with \`/\`) to avoid ambiguity.
- Don't attempt to run compilers, interpreters, or external tools — they don't exist in this environment.
- Never mention or reference host machine paths, project instructions, or runtime context in your responses.
`.trim();

export default function (pi: any) {
	// Inject bashkit context into the LLM system prompt
	pi.on("before_agent_start", async (event: any) => {
		return {
			systemPrompt: event.systemPrompt + "\n\n" + BASHKIT_SYSTEM_PROMPT,
		};
	});

	// --- bash tool ---
	pi.registerTool({
		name: "bash",
		label: "bashkit",
		description:
			"Execute bash commands in bashkit's virtual sandbox. Full bash interpreter with 100+ builtins (echo, grep, sed, awk, jq, curl, find, etc.) running in-memory. All file operations use a virtual filesystem. State persists across calls.",
		parameters: {
			type: "object",
			properties: {
				command: {
					type: "string",
					description: "Bash command to execute",
				},
				timeout: {
					type: "number",
					description: "Timeout in seconds (optional)",
				},
			},
			required: ["command"],
		},
		async execute(
			_toolCallId: string,
			params: { command: string; timeout?: number },
		) {
			const result = bash.executeSync(params.command);
			let output = "";
			if (result.stdout) output += result.stdout;
			if (result.stderr) output += result.stderr;
			if (!output) output = "(no output)";
			if (result.exitCode !== 0) {
				output += `\n\nCommand exited with code ${result.exitCode}`;
				throw new Error(output);
			}
			return {
				content: [{ type: "text", text: output }],
				details: { engine: "bashkit" },
			};
		},
	});

	// --- read tool (direct VFS) ---
	pi.registerTool({
		name: "read",
		label: "bashkit-read",
		description:
			"Read file contents from bashkit's virtual filesystem. Returns file content with line numbers.",
		parameters: {
			type: "object",
			properties: {
				path: { type: "string", description: "File path to read" },
				offset: {
					type: "number",
					description: "Line offset to start reading from (1-based)",
				},
				limit: {
					type: "number",
					description: "Maximum number of lines to return",
				},
			},
			required: ["path"],
		},
		async execute(
			_toolCallId: string,
			params: { path: string; offset?: number; limit?: number },
		) {
			const absPath = resolvePath(params.path);
			const content = bash.readFile(absPath);
			let lines = content.split("\n");

			// Remove trailing empty line if file ends with newline
			if (lines.length > 0 && lines[lines.length - 1] === "") {
				lines.pop();
			}

			const offset = (params.offset ?? 1) - 1;
			if (offset > 0) lines = lines.slice(offset);
			if (params.limit) lines = lines.slice(0, params.limit);

			const numbered = lines
				.map((line, i) => `${offset + i + 1}\t${line}`)
				.join("\n");

			return {
				content: [{ type: "text", text: numbered || "(empty file)" }],
				details: { engine: "bashkit" },
			};
		},
	});

	// --- write tool (direct VFS) ---
	pi.registerTool({
		name: "write",
		label: "bashkit-write",
		description:
			"Write file contents to bashkit's virtual filesystem. Creates parent directories automatically.",
		parameters: {
			type: "object",
			properties: {
				path: { type: "string", description: "File path to write" },
				content: {
					type: "string",
					description: "Content to write to the file",
				},
			},
			required: ["path", "content"],
		},
		async execute(
			_toolCallId: string,
			params: { path: string; content: string },
		) {
			const absPath = resolvePath(params.path);
			ensureParentDir(absPath);
			bash.writeFile(absPath, params.content);
			return {
				content: [
					{
						type: "text",
						text: `Wrote ${params.content.length} bytes to ${absPath}`,
					},
				],
				details: { engine: "bashkit" },
			};
		},
	});

	// --- edit tool (direct VFS) ---
	pi.registerTool({
		name: "edit",
		label: "bashkit-edit",
		description:
			"Edit a file in bashkit's virtual filesystem by replacing oldText with newText. The oldText must appear exactly once in the file.",
		parameters: {
			type: "object",
			properties: {
				path: { type: "string", description: "File path to edit" },
				oldText: {
					type: "string",
					description:
						"Exact text to find and replace (must be unique in file)",
				},
				newText: { type: "string", description: "Replacement text" },
			},
			required: ["path", "oldText", "newText"],
		},
		async execute(
			_toolCallId: string,
			params: { path: string; oldText: string; newText: string },
		) {
			const absPath = resolvePath(params.path);
			const content = bash.readFile(absPath);

			const count = content.split(params.oldText).length - 1;
			if (count === 0) {
				throw new Error(
					`oldText not found in ${absPath}. File content:\n${content}`,
				);
			}
			if (count > 1) {
				throw new Error(
					`oldText found ${count} times in ${absPath}. Must be unique.`,
				);
			}

			const newContent = content.replace(params.oldText, params.newText);
			bash.writeFile(absPath, newContent);

			return {
				content: [{ type: "text", text: `Edited ${absPath}` }],
				details: { engine: "bashkit" },
			};
		},
	});
}

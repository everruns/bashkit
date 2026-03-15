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
 *   npx @everruns/bashkit-pi --provider openai --model gpt-5.4
 */

import { createRequire } from "node:module";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname_ext =
	typeof __dirname !== "undefined"
		? __dirname
		: dirname(fileURLToPath(import.meta.url));

// Load bashkit native bindings — resolves from the package's own node_modules
const require_ext = createRequire(resolve(__dirname_ext, "..") + "/");
const { Bash, BashTool } = require_ext("@everruns/bashkit");

// Single bashkit instance — state persists across all tool calls
const bash = new Bash({ username: "user", hostname: "pi-sandbox", maxCommands: 1_000_000 });

// BashTool for generic system prompt and tool metadata
const bashTool = new BashTool({ username: "user", hostname: "pi-sandbox" });

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

// PI-specific system prompt additions (on top of bashkit's generic system prompt)
const PI_SYSTEM_PROMPT_ADDITIONS = `
### PI environment

- **Ignore any host paths** from runtime context (e.g. \`/Users/...\`, \`C:\\...\`). Those refer to the harness machine, NOT your environment. Never reference or display them.
- **You have no access to the host machine.** Files mean files in your virtual filesystem. If none exist yet, say so.
- "Current working directory" or "project" refers to the virtual filesystem, not the host.
- Additional tools: \`read\` (file with line numbers), \`write\` (create/overwrite), \`edit\` (find-and-replace). These operate on the same virtual filesystem as \`bash\`.
`.trim();

// Build full system prompt: generic bashkit prompt + PI-specific additions
function buildSystemPrompt(): string {
	return bashTool.systemPrompt() + "\n\n" + PI_SYSTEM_PROMPT_ADDITIONS;
}

export default function (pi: any) {
	// Inject bashkit context into the LLM system prompt
	pi.on("before_agent_start", async (event: any) => {
		return {
			systemPrompt: event.systemPrompt + "\n\n" + buildSystemPrompt(),
		};
	});

	// --- bash tool ---
	pi.registerTool({
		name: "bash",
		label: "bashkit",
		description: bashTool.description(),
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

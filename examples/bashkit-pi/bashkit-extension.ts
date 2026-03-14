/**
 * Pi extension: replaces bash, read, write, and edit tools with bashkit virtual implementations.
 *
 * Uses @everruns/bashkit Node.js bindings (NAPI-RS) — no subprocess, no Python.
 * All operations run against bashkit's in-memory virtual filesystem.
 * State (variables, files, cwd) persists across tool calls within a session.
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

// Helper: execute bash and return stdout/stderr
function execBash(command: string): {
	stdout: string;
	stderr: string;
	exitCode: number;
} {
	const result = bash.executeSync(command);
	return {
		stdout: result.stdout ?? "",
		stderr: result.stderr ?? "",
		exitCode: result.exitCode ?? 0,
	};
}

// Helper: read file via bash cat (uses the shared VFS)
function vfsRead(path: string): string {
	const result = bash.executeSync(`cat '${path.replace(/'/g, "'\\''")}'`);
	if (result.exitCode !== 0) {
		throw new Error(result.stderr || `Failed to read ${path}`);
	}
	return result.stdout ?? "";
}

// Helper: write file via bash (uses the shared VFS)
function vfsWrite(path: string, content: string): void {
	// Ensure parent dir exists
	const dir = path.replace(/\/[^/]*$/, "");
	if (dir && dir !== path) {
		bash.executeSync(`mkdir -p '${dir.replace(/'/g, "'\\''")}'`);
	}
	// Use heredoc to write content safely
	const marker = `__BASHKIT_EOF_${Date.now()}__`;
	const result = bash.executeSync(`cat > '${path.replace(/'/g, "'\\''")}' <<'${marker}'\n${content}\n${marker}`);
	if (result.exitCode !== 0) {
		throw new Error(result.stderr || `Failed to write ${path}`);
	}
}

// Helper: check if file exists
function vfsExists(path: string): boolean {
	const result = bash.executeSync(
		`test -e '${path.replace(/'/g, "'\\''")}'`,
	);
	return result.exitCode === 0;
}

// Resolve relative paths against bashkit home
function resolvePath(userPath: string): string {
	if (userPath.startsWith("/")) return userPath;
	return `/home/user/${userPath}`;
}

export default function (pi: any) {
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
			const result = execBash(params.command);
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

	// --- read tool ---
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
			const content = vfsRead(absPath);
			let lines = content.split("\n");

			// Remove trailing empty line from cat output
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

	// --- write tool ---
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
			vfsWrite(absPath, params.content);
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

	// --- edit tool ---
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
			const content = vfsRead(absPath);

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
			vfsWrite(absPath, newContent);

			return {
				content: [{ type: "text", text: `Edited ${absPath}` }],
				details: { engine: "bashkit" },
			};
		},
	});
}

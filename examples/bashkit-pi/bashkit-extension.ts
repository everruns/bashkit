/**
 * Pi extension: replaces bash, read, write, and edit tools with bashkit virtual implementations.
 *
 * All operations run against bashkit's in-memory virtual filesystem — no real FS access.
 * State (variables, files, cwd) persists across tool calls within a session.
 *
 * Usage: pi -e examples/bashkit-pi/bashkit-extension.ts
 *
 * Requires: cargo build --example pi_server --release
 */

import { spawn, type ChildProcess } from "child_process";
import { randomBytes } from "crypto";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { existsSync } from "fs";

// Resolve server binary path
const __dirname_ext =
	typeof __dirname !== "undefined"
		? __dirname
		: dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = resolve(__dirname_ext, "../..");

// Find the pi_server binary
function findServerBinary(): string {
	const candidates = [
		process.env.BASHKIT_PI_SERVER,
		resolve(PROJECT_ROOT, "target/release/examples/pi_server"),
		resolve(PROJECT_ROOT, "target/debug/examples/pi_server"),
	].filter(Boolean) as string[];

	for (const path of candidates) {
		if (existsSync(path)) return path;
	}
	throw new Error(
		`pi_server binary not found. Run: cargo build --example pi_server --release\nSearched: ${candidates.join(", ")}`,
	);
}

interface PendingRequest {
	resolve: (resp: any) => void;
	reject: (err: Error) => void;
}

let serverProcess: ChildProcess | null = null;
let pendingRequests: Map<string, PendingRequest> = new Map();
let lineBuffer = "";
let serverReady: Promise<void>;
let resolveReady: () => void;

function ensureServer(): Promise<void> {
	if (serverProcess && !serverProcess.killed) {
		return serverReady;
	}

	serverReady = new Promise((res) => {
		resolveReady = res;
	});

	const binary = findServerBinary();
	serverProcess = spawn(binary, [], {
		stdio: ["pipe", "pipe", "pipe"],
	});

	serverProcess.stdout!.on("data", (data: Buffer) => {
		lineBuffer += data.toString("utf-8");
		const lines = lineBuffer.split("\n");
		lineBuffer = lines.pop() || "";

		for (const line of lines) {
			if (!line.trim()) continue;
			try {
				const msg = JSON.parse(line);
				if (msg.ready) {
					resolveReady();
					continue;
				}
				const pending = pendingRequests.get(msg.id);
				if (pending) {
					pendingRequests.delete(msg.id);
					pending.resolve(msg);
				}
			} catch {
				// skip malformed
			}
		}
	});

	serverProcess.stderr!.on("data", (data: Buffer) => {
		process.stderr.write(`[bashkit] ${data.toString()}`);
	});

	serverProcess.on("exit", (code) => {
		serverProcess = null;
		for (const [, pending] of pendingRequests) {
			pending.reject(new Error(`bashkit server exited with code ${code}`));
		}
		pendingRequests.clear();
	});

	return serverReady;
}

function rpcCall(payload: Record<string, any>): Promise<any> {
	const id = randomBytes(8).toString("hex");
	return new Promise((resolve, reject) => {
		pendingRequests.set(id, { resolve, reject });
		serverProcess!.stdin!.write(JSON.stringify({ id, ...payload }) + "\n");
	});
}

async function execBash(
	command: string,
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
	await ensureServer();
	const resp = await rpcCall({ op: "bash", command });
	if (resp.error) throw new Error(resp.error);
	return {
		stdout: resp.stdout || "",
		stderr: resp.stderr || "",
		exitCode: resp.exit_code ?? 0,
	};
}

async function vfsRead(path: string): Promise<string> {
	await ensureServer();
	const resp = await rpcCall({ op: "read", path });
	if (resp.error) throw new Error(resp.error);
	return resp.content ?? "";
}

async function vfsWrite(path: string, content: string): Promise<void> {
	await ensureServer();
	const resp = await rpcCall({ op: "write", path, content });
	if (resp.error) throw new Error(resp.error);
}

async function vfsExists(path: string): Promise<boolean> {
	await ensureServer();
	const resp = await rpcCall({ op: "exists", path });
	if (resp.error) throw new Error(resp.error);
	return resp.exists ?? false;
}

async function vfsMkdir(path: string): Promise<void> {
	await ensureServer();
	const resp = await rpcCall({ op: "mkdir", path });
	if (resp.error) throw new Error(resp.error);
}

// Resolve path relative to virtual cwd (always /home/user in bashkit)
function resolvePath(userPath: string): string {
	if (userPath.startsWith("/")) return userPath;
	return `/home/user/${userPath}`;
}

export default function (pi: any) {
	// --- bash tool (replaces built-in) ---
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
			_signal: AbortSignal,
			_onUpdate: (update: any) => void,
		) {
			const result = await execBash(params.command);
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

	// --- read tool (replaces built-in) ---
	pi.registerTool({
		name: "read",
		label: "bashkit-read",
		description:
			"Read file contents from bashkit's virtual filesystem. Returns file content with line numbers.",
		parameters: {
			type: "object",
			properties: {
				path: {
					type: "string",
					description: "File path to read",
				},
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
			const content = await vfsRead(absPath);
			let lines = content.split("\n");

			// Apply offset/limit
			const offset = (params.offset ?? 1) - 1;
			if (offset > 0) lines = lines.slice(offset);
			if (params.limit) lines = lines.slice(0, params.limit);

			// Add line numbers
			const numbered = lines
				.map((line, i) => `${offset + i + 1}\t${line}`)
				.join("\n");

			return {
				content: [{ type: "text", text: numbered || "(empty file)" }],
				details: { engine: "bashkit" },
			};
		},
	});

	// --- write tool (replaces built-in) ---
	pi.registerTool({
		name: "write",
		label: "bashkit-write",
		description:
			"Write file contents to bashkit's virtual filesystem. Creates parent directories automatically.",
		parameters: {
			type: "object",
			properties: {
				path: {
					type: "string",
					description: "File path to write",
				},
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
			await vfsWrite(absPath, params.content);
			return {
				content: [
					{ type: "text", text: `Wrote ${params.content.length} bytes to ${absPath}` },
				],
				details: { engine: "bashkit" },
			};
		},
	});

	// --- edit tool (replaces built-in) ---
	pi.registerTool({
		name: "edit",
		label: "bashkit-edit",
		description:
			"Edit a file in bashkit's virtual filesystem by replacing oldText with newText. The oldText must appear exactly once in the file.",
		parameters: {
			type: "object",
			properties: {
				path: {
					type: "string",
					description: "File path to edit",
				},
				oldText: {
					type: "string",
					description: "Exact text to find and replace (must be unique in file)",
				},
				newText: {
					type: "string",
					description: "Replacement text",
				},
			},
			required: ["path", "oldText", "newText"],
		},
		async execute(
			_toolCallId: string,
			params: { path: string; oldText: string; newText: string },
		) {
			const absPath = resolvePath(params.path);
			const content = await vfsRead(absPath);

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
			await vfsWrite(absPath, newContent);

			return {
				content: [{ type: "text", text: `Edited ${absPath}` }],
				details: { engine: "bashkit" },
			};
		},
	});

	// Cleanup on exit
	pi.on("session_end", async () => {
		if (serverProcess && !serverProcess.killed) {
			serverProcess.kill();
			serverProcess = null;
		}
	});
}

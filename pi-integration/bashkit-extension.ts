/**
 * Pi extension: replaces the built-in bash tool with bashkit's virtual bash interpreter.
 *
 * All commands run in bashkit's sandboxed virtual filesystem — no real filesystem access.
 * State (variables, files, cwd) persists across tool calls within a session.
 *
 * Install: pi -e /path/to/bashkit/pi-integration/bashkit-extension.ts
 */

import { spawn, type ChildProcess } from "child_process";
import { randomBytes } from "crypto";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

// Resolve server script path relative to this extension
const __dirname_ext =
	typeof __dirname !== "undefined"
		? __dirname
		: dirname(fileURLToPath(import.meta.url));
const SERVER_SCRIPT = resolve(__dirname_ext, "bashkit_server.py");

// Find python in venv or PATH
const PYTHON =
	process.env.BASHKIT_PYTHON || "/home/user/.venv/bin/python3" || "python3";

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

	serverReady = new Promise((resolve) => {
		resolveReady = resolve;
	});

	serverProcess = spawn(PYTHON, [SERVER_SCRIPT], {
		stdio: ["pipe", "pipe", "pipe"],
		env: { ...process.env, PYTHONUNBUFFERED: "1" },
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
				// skip malformed lines
			}
		}
	});

	serverProcess.stderr!.on("data", (data: Buffer) => {
		process.stderr.write(`[bashkit-server] ${data.toString()}`);
	});

	serverProcess.on("exit", (code) => {
		serverProcess = null;
		// Reject all pending requests
		for (const [id, pending] of pendingRequests) {
			pending.reject(new Error(`bashkit server exited with code ${code}`));
		}
		pendingRequests.clear();
	});

	return serverReady;
}

async function execInBashkit(
	command: string,
	timeoutMs?: number,
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
	await ensureServer();

	const id = randomBytes(8).toString("hex");
	const req = { id, command, timeout_ms: timeoutMs };

	return new Promise((resolve, reject) => {
		pendingRequests.set(id, {
			resolve: (resp) =>
				resolve({
					stdout: resp.stdout || "",
					stderr: resp.stderr || "",
					exitCode: resp.exit_code ?? 0,
				}),
			reject,
		});

		serverProcess!.stdin!.write(JSON.stringify(req) + "\n");
	});
}

export default function (pi: any) {
	// Register our bashkit-powered bash tool, replacing the built-in
	pi.registerTool({
		name: "bash",
		label: "bashkit",
		description: `Execute bash commands in bashkit's virtual sandbox. Provides a full bash interpreter with 100+ builtins (echo, grep, sed, awk, jq, curl, find, etc.) running entirely in-memory. All file operations use a virtual filesystem — no real filesystem access. State persists across calls.`,
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
			toolCallId: string,
			params: { command: string; timeout?: number },
			signal: AbortSignal,
			onUpdate: (update: any) => void,
		) {
			const { command, timeout } = params;
			const timeoutMs = timeout ? timeout * 1000 : undefined;

			try {
				const result = await execInBashkit(command, timeoutMs);

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
					details: { virtual: true, engine: "bashkit" },
				};
			} catch (err: any) {
				throw err;
			}
		},
	});

	// Cleanup on session end
	pi.on("session_end", async () => {
		if (serverProcess && !serverProcess.killed) {
			serverProcess.kill();
			serverProcess = null;
		}
	});
}

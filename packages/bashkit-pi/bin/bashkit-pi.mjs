#!/usr/bin/env node

/**
 * npx @everruns/bashkit-pi launcher
 *
 * Downloads/locates the bashkit pi extension and invokes pi with it.
 * All CLI args are forwarded to pi.
 *
 * Usage:
 *   npx @everruns/bashkit-pi --provider openai --model gpt-5.4 --api-key "$OPENAI_API_KEY"
 *   npx @everruns/bashkit-pi --provider anthropic --model claude-sonnet-4-20250514
 */

import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const extensionPath = resolve(__dirname, "..", "extension", "bashkit-extension.ts");

if (!existsSync(extensionPath)) {
	console.error(`Error: extension not found at ${extensionPath}`);
	process.exit(1);
}

// Find pi binary — check npx-installed, global, or PATH
function findPi() {
	const locations = [
		// npm global
		"pi",
	];

	for (const loc of locations) {
		try {
			execFileSync("which", [loc], { stdio: "pipe" });
			return loc;
		} catch {
			// not found, try next
		}
	}
	return null;
}

const pi = findPi();
if (!pi) {
	console.error("Error: pi not found. Install with: npm install -g @mariozechner/pi-coding-agent");
	process.exit(1);
}

// Forward all args to pi, injecting -e <extension>
const userArgs = process.argv.slice(2);
const args = ["-e", extensionPath, ...userArgs];

try {
	execFileSync(pi, args, { stdio: "inherit" });
} catch (err) {
	// pi exited with non-zero — exit code already propagated via stdio: "inherit"
	process.exit(err.status ?? 1);
}

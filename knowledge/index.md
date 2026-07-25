---
okf_version: "0.2"
---

# Bashkit Knowledge

* [Knowledge Maintenance Contract](knowledge-contract.md) - Rules for maintaining the Bashkit knowledge bundle and its OKF conformance.
* [Update Log](log.md) - Chronological history of changes to this bundle.

# Foundations

* [Bashkit Architecture](architecture.md) - Core interpreter architecture, module boundaries, execution flow, and design principles.
* [Parser](parser.md) - Bash syntax parser and lexer architecture and compatibility decisions.
* [Virtual Filesystem](vfs.md) - Filesystem abstraction, path safety, implementations, and sandbox invariants.
* [Builtin Commands](builtins.md) - Builtin command trait, execution planning, registration, and implementation conventions.
* [Parallel Execution](parallel-execution.md) - Threading model, shared ownership, and concurrency safety requirements.

# Security

* [Threat Model](threat-model.md) - Bashkit assets, trust boundaries, threats, mitigations, and stable threat identifiers.
* [Security Testing](security-testing.md) - Fail-point injection and layered security regression testing strategy.
* [Credential Injection](credential-injection.md) - Per-host HTTP credential injection without exposing secret values to sandboxed scripts.
* [Request Signing](request-signing.md) - Transparent Ed25519 HTTP message signing according to RFC 9421.
* [HTTP Transport](http-transport.md) - Pluggable host-controlled HTTP transport for curl and wget.

# Runtimes and packages

* [Python Builtin](python-builtin.md) - Embedded Python execution through Monty with security and resource controls.
* [ZapCode Runtime](zapcode-runtime.md) - Embedded TypeScript runtime, external functions, VFS bridging, and resource limits.
* [SQLite Builtin](sqlite-builtin.md) - Embedded SQLite through Turso with memory and virtual filesystem backends.
* [Coreutils Argument Port](coreutils-args-port.md) - Code generation design for porting uutils clap arguments and uucore modules.
* [Python Package](python-package.md) - Python bindings, PyPI wheels, ABI strategy, and platform build matrix.
* [Emscripten Wheels](emscripten-wheels.md) - Reduced-feature Pyodide and Emscripten Python wheel design and build constraints.
* [Browser Package](browser-package.md) - Slim single-threaded WebAssembly package design for browsers and JavaScript runtimes.

# Integrations

* [Tool Contract](tool-contract.md) - Public LLM tool trait behavior, schemas, callbacks, and error semantics.
* [Scripted Tool Orchestration](scripted-tool-orchestration.md) - Composition of tool definitions and callbacks into Bash-scripted orchestrators.
* [Git Support](git-support.md) - Sandboxed Git operations over the virtual filesystem.
* [SSH Support](ssh-support.md) - Sandboxed SSH, SCP, and SFTP operations and security boundaries.
* [Interactive Shell](interactive-shell.md) - Interactive REPL design with rustyline-based line editing.

# Quality and operations

* [Testing Strategy](testing.md) - Test organization, patterns, fixtures, differential testing, and CI expectations.
* [Known Limitations](limitations.md) - Intentional gaps, partial features, and Bash and POSIX compatibility stance.
* [Documentation Architecture](documentation.md) - User documentation and Rustdoc guide organization, embedding, and maintenance.
* [Maintenance](maintenance.md) - Pre-release dependency, security, compatibility, and artifact maintenance requirements.
* [Release Process](release-process.md) - Versioning, validation, tagging, and publication to crates.io, PyPI, and npm.
* [Performance Results](performance-results.md) - Benchmark harnesses, result locations, naming, and publication contract.
* [Evaluation Framework](eval.md) - LLM evaluation study design, dataset format, execution, and scoring.

# Subdirectories

* [status/](status/) - Machine-generated inventories that capture the current Bashkit implementation state.

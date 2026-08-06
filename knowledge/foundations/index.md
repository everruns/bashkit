# Foundations

* [Bashkit Architecture](architecture.md) - Core interpreter architecture, module boundaries, execution flow, and design principles.
* [Parser](parser.md) - Bash syntax parser and lexer architecture and compatibility decisions.
* [Virtual Filesystem](vfs.md) - Filesystem abstraction, path safety, implementations, and sandbox invariants.
* [Builtin Commands](builtins.md) - Builtin command trait, execution planning, registration, and implementation conventions.
* [jq Input Compatibility](jq.md) - jq JSON input normalization, strict-boundary scope, and resource-accounting rules.
* [Parallel Execution](parallel-execution.md) - Threading model, shared ownership, and concurrency safety requirements.
* [Snapshot History and Deltas](snapshot-history.md) - Content-addressed snapshot objects, commit DAG with forks, chunked binary content, and the version and capability compatibility rules for restore.
* [Execution Profiles](execution-profiles.md) - Typed policy bundles across execution, memory, filesystem, network, and embedded runtimes.

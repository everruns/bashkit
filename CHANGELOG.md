# Changelog

All notable changes to BashKit will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-02-02

### Added

- Initial release of BashKit sandboxed bash interpreter
- Core interpreter with bash-compatible syntax support
- Virtual filesystem (VFS) abstraction for sandboxed file operations
- Resource limits: memory, execution time, operation count
- Built-in commands: echo, printf, cat, head, tail, wc, grep, sed, awk, jq, sort, uniq, cut, tr, date, base64, md5sum, sha256sum, gzip, gunzip
- Variable expansion with full bash compatibility ($var, ${var}, ${var:-default}, etc.)
- Arithmetic expansion: $((expr))
- Command substitution: $(cmd) and backticks
- Pipelines and redirections
- Control flow: if/then/else/elif/fi, for/do/done, while/do/done, case/esac
- Functions with local variables
- Arrays (indexed and associative)
- Subshells: (cmd)
- Background jobs: cmd &
- Logical operators: &&, ||
- Fuel-based operation limits to prevent DoS
- CLI tool for running scripts and interactive REPL
- Comprehensive test suite with property-based testing
- Security testing with fail-point injection
- Examples for common use cases and LLM agent integration

### Security

- Sandboxed execution with no real filesystem access by default
- Configurable resource limits prevent runaway scripts
- Virtual filesystem prevents path traversal attacks
- Memory limits prevent allocation-based DoS
- Operation limits prevent infinite loops

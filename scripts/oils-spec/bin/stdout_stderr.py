#!/usr/bin/env bash
# Shell stand-in for Oils' spec/bin/stdout_stderr.py: prints $1 (STDOUT) to
# stdout, $2 (STDERR) to stderr, exits with $3 (0).
printf '%s\n' "${1-STDOUT}"
printf '%s\n' "${2-STDERR}" >&2
exit "${3-0}"

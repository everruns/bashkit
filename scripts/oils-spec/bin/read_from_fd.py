#!/usr/bin/env bash
# Shell stand-in for Oils' spec/bin/read_from_fd.py: for each fd, prints
# "<fd>: " followed by what can be read from it.
for fd in "$@"; do
  if ! { true <&"$fd"; } 2>/dev/null; then
    echo "FATAL: Error reading from fd $fd" >&2
    exit 1
  fi
  printf '%d: ' "$fd"
  cat <&"$fd"
done

#!/usr/bin/env bash
# Shell stand-in for Oils' spec/bin/printenv.py: one line per name, the
# exported value or None.
for name in "$@"; do
  if [[ $name =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] && [[ -v $name ]]; then
    printf '%s\n' "${!name}"
  else
    echo None
  fi
done

#!/usr/bin/env bash
# Shell stand-in for Oils' spec/bin/argv.py (a Python 2 script): prints the
# arguments as a Python 2 list repr, byte for byte. Written in bash so the same
# helper runs under real bash and inside the bashkit sandbox, which does not
# execute host Python. Bytes come from od, so non-UTF-8 and multibyte
# arguments render as Python 2 would ('\xce\xbc').
out='['
sep=''
for arg in "$@"; do
  q="'"
  if [[ $arg == *"'"* && $arg != *'"'* ]]; then
    q='"'
  fi
  s=''
  for h in $(printf '%s' "$arg" | od -An -v -tx1); do
    n=$((16#$h))
    if ((n == 92)); then
      s+='\\'
    elif ((n == 39)) && [[ $q == "'" ]]; then
      s+="\\'"
    elif ((n == 9)); then
      s+='\t'
    elif ((n == 10)); then
      s+='\n'
    elif ((n == 13)); then
      s+='\r'
    elif ((n < 32 || n > 126)); then
      s+="\\x$h"
    else
      s+=$(printf "\\x$h")
    fi
  done
  out+="$sep$q$s$q"
  sep=', '
done
printf '%s]\n' "$out"

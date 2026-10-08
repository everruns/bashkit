#!/usr/bin/env bash
set -euo pipefail

readonly OUT_DIR="build"
declare -A counts=()
files=()

usage() { echo "uso: $0 [-v] arquivos..." >&2; return 1; }

process_file() {
  local file="$1" lines words
  [[ -f "$file" ]] || { echo "ignorando $file" >&2; return 0; }
  lines=$(wc -l < "$file")
  words=$(wc -w < "$file")
  counts["$file"]=$lines
  printf '%-12s %3d linhas %3d palavras\n' "$(basename "$file")" "$lines" "$words"
}

mkdir -p "$OUT_DIR"
for f in src/*.txt missing.txt; do
  files+=("$f")
done

for f in "${files[@]}"; do process_file "$f"; done

total=0
for k in "${!counts[@]}"; do total=$(( total + counts[$k] )); done
echo "total=$total arquivos=${#counts[@]}" | tee "$OUT_DIR/summary.txt"

if [[ $total -gt 3 ]]; then
  echo "grande" > "$OUT_DIR/flag"
fi
ls "$OUT_DIR"

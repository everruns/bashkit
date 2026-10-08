find . -type f -name '*.c' -print0 | sort -z | while IFS= read -r -d '' f; do
  printf 'arquivo: %q\n' "$f"
done
for f in ./*.c; do [ -e "$f" ] || continue; echo "glob: $f"; done

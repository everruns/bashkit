set -euo pipefail
IFS=$'\n\t'
SCRIPT_NAME=deploy
log() { printf '[%s] %s\n' "$SCRIPT_NAME" "$*" >&2; }
cleanup() { log "limpando"; rm -f tmp.*; }
trap cleanup EXIT
log "início"
touch tmp.a tmp.b
count=$(ls tmp.* | wc -l)
log "temporários: $count"
grep -q "porta" config.ini && log "tem porta"
porta=$(grep -E '^porta=' config.ini | cut -d= -f2)
echo "porta=$porta"
grep -q inexistente config.ini
echo nunca

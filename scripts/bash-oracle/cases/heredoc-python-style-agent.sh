cat > script.sh <<'SCRIPT'
#!/bin/bash
set -euo pipefail
echo "args: $*"
SCRIPT
chmod +x script.sh
./script.sh um dois
bash script.sh três

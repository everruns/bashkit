set -E
trap 'echo "ERR em ${FUNCNAME[0]:-main}"' ERR
g() { false; }
g
set +E
trap - ERR
trap -p
echo fim

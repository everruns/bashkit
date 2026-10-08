trap 'echo pai-sai' EXIT
( trap 'echo filho-sai' EXIT; echo no-filho )
( echo "subshell herda? $(trap -p EXIT | wc -l)" )
trap - EXIT
echo fim

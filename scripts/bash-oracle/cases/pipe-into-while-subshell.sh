total=0
printf '1\n2\n3\n' | while read -r n; do total=$((total + n)); done
echo "fora do pipe: $total"
shopt -s lastpipe
set +m
printf '1\n2\n3\n' | while read -r n; do total=$((total + n)); done
echo "lastpipe: $total"

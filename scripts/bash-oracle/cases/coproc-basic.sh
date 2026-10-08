coproc UPPER { tr a-z A-Z; }
echo "texto" >&"${UPPER[1]}"
exec {UPPER[1]}>&-
read -r line <&"${UPPER[0]}"
echo "$line"
wait

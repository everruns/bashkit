mapfile -t linhas < dados.txt
echo "${#linhas[@]}: ${linhas[1]}"
readarray -t -s 1 -n 2 parte < dados.txt
echo "${parte[*]}"
mapfile -t -d , campos <<< "a,b,c"
printf '[%s]' "${campos[@]}"; echo
mapfile arr < <(printf 'x\ny\n'); printf '%q ' "${arr[@]}"; echo

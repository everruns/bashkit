echo *.txt
echo *.nada
shopt -s nullglob
echo "nullglob:[" *.nada "]"
arr=(*.log); echo "${#arr[@]}"
shopt -u nullglob
shopt -s failglob
echo *.nada
echo "rc=$?"

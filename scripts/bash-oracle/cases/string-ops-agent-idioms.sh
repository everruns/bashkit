s="  Olá, Mundo!  "
trimmed="${s#"${s%%[![:space:]]*}"}"; trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
echo "[$trimmed]"
csv="a,b,,d"
IFS=, read -ra parts <<< "$csv"; echo "${#parts[@]}"
str="abc"; for ((i=${#str}-1; i>=0; i--)); do rev+="${str:i:1}"; done; echo "$rev"
[[ "$s" == *Mundo* ]] && echo contém
echo "${#trimmed}"
printf '%s\n' "$trimmed" | wc -c

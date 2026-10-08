a=(zero um "dois três")
a+=(quatro)
a[10]=dez
echo "${#a[@]} ${a[2]} ${a[-1]} ${!a[*]}"
unset 'a[1]'
echo "${a[@]}"
for x in "${a[@]}"; do printf '[%s]' "$x"; done; echo
b=("${a[@]:1:2}"); echo "${b[@]}"
c=($(echo x y z)); echo "${#c[@]}"
echo "${a[*]}" "${a[@]/#/-}"
sorted=($(printf '%s\n' 3 1 2 | sort -n)); echo "${sorted[*]}"

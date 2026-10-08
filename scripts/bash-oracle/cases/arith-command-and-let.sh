x=5
if (( x > 3 )); then echo maior; fi
(( x == 0 )) || echo "não é zero"
(( 0 )); echo "rc0=$?"
let "y = x * 2" z=x+1; echo "$y $z"
declare -i n; n="2 + 3"; echo $n
for ((i = 0; i < 3; i++)); do printf '%d,' "$i"; done; echo

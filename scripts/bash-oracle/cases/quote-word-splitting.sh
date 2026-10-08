v="a   b    c"
printf '[%s]\n' $v
printf '[%s]\n' "$v"
IFS=: read -r x y z <<< "1:2:3"
echo "$x-$y-$z"
IFS=,; set -- $(echo "p,q,r"); echo "$#: $2"

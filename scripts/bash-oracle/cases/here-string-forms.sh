tr a-z A-Z <<< "minúsculas ascii"
read -r a b <<< "primeiro segundo terceiro"
echo "$a|$b"
wc -l <<< ""
x=42; bc_like=$(cat <<< "$x")
echo "$bc_like"

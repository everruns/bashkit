files=$(printf '%s\n' "a b" "c")
n=$(( $(echo "$files" | wc -l) * 2 ))
echo "n=$n"
echo "$(printf '%s' "$(echo "x  y")")"
echo $(( $(echo 3) + $(echo $(( 2 * $(echo 4) ))) ))
msg="contagem: $(echo "$files" | grep -c "a b")"
echo "$msg"

s="version-1.2.3-beta"
if [[ $s =~ ([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
  echo "match=${BASH_REMATCH[0]} major=${BASH_REMATCH[1]} n=${#BASH_REMATCH[@]}"
fi
re='^[a-z]+-[0-9]'
[[ $s =~ $re ]] && echo "var-regex ok"
[[ $s =~ "1.2" ]] && echo "quoted literal ok"
[[ $s == version-* ]] && echo glob-ok
[[ $s == "version-*" ]] || echo "quoted não é glob"
[[ -n $s && ( $s == *beta || $s == *alpha ) ]] && echo grouping-ok
[[ "b" > "a" ]] && echo lexico-ok
[[ 10 -gt 9 ]] && echo num-ok
x=""; [[ -z $x ]] && echo "sem aspas ok"

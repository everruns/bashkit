s="ação ñ 日本"
echo "${#s}"
echo "$s" | wc -c
printf '%s\n' "${s:0:4}"
echo "${s^^}"
LC_ALL=C; t="ação"; echo "${#t}"

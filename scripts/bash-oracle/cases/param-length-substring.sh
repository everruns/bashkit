s="abcdefghij"
echo "${#s} ${s:2} ${s:2:3} ${s: -3} ${s: -4:2} ${s:0:0}|"
a=(um dois três quatro)
echo "${#a[@]} ${#a[2]} ${a[@]:1:2}"
set -- p1 p2 p3 p4
echo "${@:2} | ${@:2:2} | ${#}"

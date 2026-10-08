f() { echo out; echo err >&2; }
f > o1.txt 2> e1.txt
f > both.txt 2>&1
f 2>&1 > only_out.txt | sed 's/^/pipe:/'
f &> amp.txt
f &>> amp.txt
f >> o1.txt 2>/dev/null
f 2>&1 | wc -l
for x in o1 e1 both only_out amp; do printf '%s: ' "$x"; tr '\n' ' ' < $x.txt; echo; done

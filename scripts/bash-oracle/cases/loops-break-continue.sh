for i in 1 2 3 4 5 6; do
  (( i == 2 )) && continue
  (( i == 5 )) && break
  echo "for $i"
done
n=0
while true; do n=$((n + 1)); [ $n -ge 3 ] && break; done; echo "while $n"
until [ $n -le 0 ]; do n=$((n - 1)); done; echo "until $n"
for i in 1 2; do for j in a b c; do [ $j = b ] && continue 2; echo "$i$j"; done; done
for x; do echo "nunca"; done
set -- p q; for x; do echo "pos $x"; done

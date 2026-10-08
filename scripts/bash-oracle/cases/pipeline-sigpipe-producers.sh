yes | head -n 2
seq 1 100000 | head -n 1
cat /dev/zero | head -c 5 | od -An -c
echo "${PIPESTATUS[*]}"

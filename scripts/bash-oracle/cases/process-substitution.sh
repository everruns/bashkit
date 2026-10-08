diff <(printf 'a\nb\n') <(printf 'a\nc\n')
echo "rc=$?"
while read -r l; do echo "lido:$l"; done < <(printf 'x\ny\n')
paste <(seq 3) <(seq 4 6)
cat <(echo "fd: ok")

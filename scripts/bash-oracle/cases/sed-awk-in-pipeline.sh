printf 'nome=ana\nidade=30\n' > p.env
sed -n 's/^nome=//p' p.env
awk -F= '{ printf "%s -> %s\n", $1, $2 }' p.env
sed -i 's/30/31/' p.env && cat p.env
awk 'BEGIN { s = 0 } { s += length($0) } END { print s }' p.env

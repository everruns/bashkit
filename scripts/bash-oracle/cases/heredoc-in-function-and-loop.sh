gen() {
  local n=$1
  cat <<EOF
item-$n
EOF
}
for i in 1 2 3; do gen "$i"; done
while read -r line; do echo "<$line>"; done <<EOF
x y
  z
EOF

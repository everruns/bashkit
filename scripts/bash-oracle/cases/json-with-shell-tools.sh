cat > dados.json <<'EOF'
{"items": [{"n": "a", "v": 1}, {"n": "b", "v": 2}]}
EOF
jq -r '.items[] | "\(.n)=\(.v)"' dados.json
total=$(jq '[.items[].v] | add' dados.json)
echo "total=$total"
jq -n --arg k chave --argjson v 3 '{($k): $v}'

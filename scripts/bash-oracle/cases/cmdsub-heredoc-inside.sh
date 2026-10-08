msg=$(cat <<EOF
linha 1
linha $((1 + 1))
EOF
)
echo "$msg"
json=$(cat <<'JSON'
{"a": [1, 2, 3]}
JSON
)
echo "${#json}"

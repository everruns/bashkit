cat > config.json <<'EOF'
{"name": "app", "port": 8080}
EOF
cat <<EOF | tr a-z A-Z
linha um
linha dois
EOF
wc -c < config.json

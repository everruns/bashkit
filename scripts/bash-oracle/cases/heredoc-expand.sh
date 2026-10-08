user=agente
cat <<EOF
olá $user
soma $((2 + 3))
cmd $(echo sub)
literal \$user e \`crase\`
EOF

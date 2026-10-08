cat <<'EOF'
nada expande: $HOME $(whoami) `id` $((1+1))
barra \n fica
EOF
cat <<"END"
também $HOME
END

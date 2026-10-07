### heredoc_backslash_escapes
user=agente
cat <<EOF
literal \$user e \`crase\` back\\slash \n keep
join \
ed $(echo "a\$b") $((1+\
2))
`echo bt`
EOF
### expect
literal $user e `crase` back\slash \n keep
join ed a$b 3
bt
### end

### heredoc_quoted_delimiter_stays_raw
cat <<'Q'
raw \$user `x`
Q
### expect
raw \$user `x`
### end

### read_without_r_removes_backslashes
read a <<< 'barra\invertida'; echo "$a"
read -r a <<< 'barra\invertida'; echo "$a"
read a b <<< 'x\ y z'; echo "[$a] [$b]"
read <<< 'r\e\p'; echo "$REPLY"
### expect
barrainvertida
barra\invertida
[x y] [z]
rep
### end

### read_status_at_eof
read nada < /dev/null; echo "rc=$? [$nada]"
printf 'abc' | { read v; echo "rc=$? $v"; }
read -d ';' x <<< "semfim"; echo "rc=$? $x"
read -n 5 y <<< "ab"; echo "rc=$? $y"
read -n 3 y <<< "abcdef"; echo "rc=$? $y"
### expect
rc=1 []
rc=1 abc
rc=1 semfim
rc=0 ab
rc=0 abc
### end

### read_continuation_from_pipe
printf 'l1\\\nl2\nl3\n' | { read v; echo "$v"; read w; echo "$w"; }
printf 'a;b;c' | { read -d ';' p; read -d ';' q; echo "$p $q"; }
### expect
l1l2
l3
a b
### end

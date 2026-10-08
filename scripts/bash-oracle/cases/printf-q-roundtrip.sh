s=$'aspas "duplas" e \'simples\' e $cifrao e tab\t e nl\n'
printf '%q\n' "$s"
printf '%q\n' "simples" "com espaço" "" "a*b" "~home"
eval "t=$(printf '%q' "$s")"
[[ "$t" == "$s" ]] && echo roundtrip-ok

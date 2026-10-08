declare -A cor=([maçã]=vermelha [banana]=amarela)
cor[uva]=roxa
cor+=([kiwi]=verde)
echo "${cor[banana]} ${#cor[@]}"
for k in $(printf '%s\n' "${!cor[@]}" | sort); do echo "$k=${cor[$k]}"; done
[[ -v cor[uva] ]] && echo "tem uva"
unset 'cor[uva]'
[[ -v cor[uva] ]] || echo "sem uva"
declare -A conta
for w in a b a c a b; do conta[$w]=$(( ${conta[$w]:-0} + 1 )); done
for k in a b c; do echo "$k:${conta[$k]}"; done

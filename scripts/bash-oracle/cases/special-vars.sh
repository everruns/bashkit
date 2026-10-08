f() { echo "$0|$#|$*|$@|${10:-}"; }
f a b c d e f g h i j
echo "$-" | grep -q h && echo "hashall"
echo "${BASH_VERSINFO[0]}"
echo "$SHLVL"
echo "$LINENO"
echo "${#BASH_ARGV[@]}"

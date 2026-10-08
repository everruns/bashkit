PS4='+ [${LINENO}] '
set -x
x=$(echo sub)
[[ $x == sub ]] && echo ok

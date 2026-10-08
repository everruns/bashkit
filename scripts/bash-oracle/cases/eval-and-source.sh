cmd='echo "eval: $((1 + 2))"'
eval "$cmd"
var=nome; eval "$var=valor"; echo "$nome"
source ./lib.sh
. ./lib.sh arg1
echo "$LIB_LOADED $(helper)"

set -- a b c d e
echo "$# $1"
shift; echo "$# $1"
shift 2; echo "$# $*"
shift 5; echo "rc=$? $#"
set -- "x y" z
for a in "$@"; do echo "[$a]"; done
for a in $*; do echo "<$a>"; done
IFS=-; echo "$*"; unset IFS
set --; echo "vazio=$#"

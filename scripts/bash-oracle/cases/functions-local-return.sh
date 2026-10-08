contador=0
inc() { local passo=${1:-1}; contador=$((contador + passo)); return 3; }
inc 5; echo "rc=$? contador=$contador"
outer() { local x=outer; inner; echo "outer vê $x"; }
inner() { echo "inner vê $x"; x=mudado; }
x=global; outer; echo "global $x"
function com_keyword { echo "args=$# primeiro=$1"; }
com_keyword a "b c"
fat() { (( $1 <= 1 )) && { echo 1; return; }; echo $(( $1 * $(fat $(( $1 - 1 ))) )); }
fat 10
f() { local -a arr=(1 2 3); local -A m=([k]=v); echo "${#arr[@]} ${m[k]}"; }; f
declare -f inc | head -2

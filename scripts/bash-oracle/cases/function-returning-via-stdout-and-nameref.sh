get_ext() { echo "${1##*.}"; }
ext=$(get_ext foto.jpeg); echo "$ext"
set_result() { local -n out=$1; out="via nameref"; }
set_result r; echo "$r"
append() { local -n arr_ref=$1; arr_ref+=("$2"); }
lista=(); append lista x; append lista y; echo "${lista[*]}"

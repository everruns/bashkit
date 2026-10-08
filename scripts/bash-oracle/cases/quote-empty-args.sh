f() { echo "$#"; }
e=""
f $e
f "$e"
f "" ""
f "${unset_var}"

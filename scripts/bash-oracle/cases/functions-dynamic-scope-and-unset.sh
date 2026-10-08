f() { local v=f; g; }
g() { echo "g vê ${v:-nada}"; unset v; echo "após unset ${v:-nada}"; }
v=global; f; echo "global: $v"
h() { echo "$FUNCNAME ${#FUNCNAME[@]}"; }; h
type h | head -1
unset -f h; type h 2>&1

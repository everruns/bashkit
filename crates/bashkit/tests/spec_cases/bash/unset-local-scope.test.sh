### unset_local_exposes_global
f() { local v=f; g; echo "f sees ${v:-none}"; }
g() { echo "g sees ${v:-none}"; unset v; echo "after unset ${v:-none}"; }
v=global; f; echo "global: $v"
### expect
g sees f
after unset global
f sees global
global: global
### end

### unset_global_from_function
f() { unset v; echo "in f ${v:-none}"; }
v=global; f; echo "after ${v:-none}"
### expect
in f none
after none
### end

### unset_local_in_own_function
f() { local v=f; unset v; echo "${v:-none}"; }
v=global; f; echo "$v"
### expect
none
global
### end

### unset_local_array_exposes_global
f() { local -a a=(x y); g; echo "f: ${a[*]}"; }
g() { unset a; echo "g: ${a[*]:-none}"; }
a=(1 2 3); f; echo "global: ${a[*]}"
### expect
g: 1 2 3
f: 1 2 3
global: 1 2 3
### end

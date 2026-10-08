### attr_transform_scalar_flags
declare -irx a=1
echo "${a@a}"
declare -u up=x
echo "${up@a}"
plain=1
echo "[${plain@a}]"
echo "[${never_set@a}]"
### expect
irx
u
[]
[]
### end

### attr_transform_arrays
declare -alr b=(X)
echo "${b@a}"
declare -a e
echo "[${e@a}]"
declare -A f=([k]=v)
echo "${f[k]@a} ${f@a}"
g=(1 2)
echo "${g[@]@a}|${g[1]@a}"
### expect
arl
[a]
A A
a a|a
### end

### attr_transform_follows_nameref
declare -i target=3
declare -n ref=target
echo "${ref@a}"
### expect
i
### end

### attr_transform_local_integer
f() { local -i z=1; echo "${z@a}"; }
f
### expect
i
### end

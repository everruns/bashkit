# `$name` on an array is element 0; PIPESTATUS after simple commands;
# platform identity variables scripts branch on.

### array_scalar_access_is_element_zero
a=(x y z)
echo "[$a]"
declare -A m=([0]=zero [k]=v)
echo "[$m]"
f() { local -a b=(p q); echo "[$b]"; }
f
### expect
[x]
[zero]
[p]
### end

### funcname_scalar
outer() { echo "[$FUNCNAME]"; inner; }
inner() { echo "[$FUNCNAME][${FUNCNAME[*]}]"; }
outer
### expect
[outer]
[inner][inner outer]
### end

### pipestatus_after_simple_command
true
echo "[${PIPESTATUS[@]}]"
false
echo "[${PIPESTATUS[@]}]"
true | false | true
echo "[${PIPESTATUS[@]}]"
### expect
[0]
[1]
[0 1 0]
### end

### ostype_and_friends_set
case "$OSTYPE" in linux*) echo linux;; *) echo "other:$OSTYPE";; esac
[ -n "$MACHTYPE" ] && [ -n "$HOSTTYPE" ] && echo machine
[ -n "$PATH" ] && echo path
### bash_diff: host bash may run on a non-Linux CI image
### expect
linux
machine
path
### end

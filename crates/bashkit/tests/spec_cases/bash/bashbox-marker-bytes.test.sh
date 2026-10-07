# BashBox marker-bytes cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_marker_bytes_values_holding_the_bytes_the_expander_uses_internally_surviv
# values holding the bytes the expander uses internally survive expansion
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
v=$(printf 'a\001b\002c\003d\004e\005f\006g\177h'); show() { printf '%s' "$1" | tr '\001\002\003\004\005\006\177' '1234567'; echo; }
show "$v"; echo ${#v}
for w in $v; do show "[$w]"; done
set -- "$v" x; show "<$1><$2>$#"
y=${v#a}; show "$y"; show "${v%g*}"; show "${v/c?d/-}"; show "${v^^}"
case "$v" in a*g?h) echo matched;; esac
arr=("$v" "q"); show "${arr[0]}|${arr[1]}"; show "${arr[*]}"
z="${v:-x}"; [[ $z == "$v" ]] && echo same
### expect
a1b2c3d4e5f6g7h
15
[a1b2c3d4e5f6g7h]
<a1b2c3d4e5f6g7h><x>2
1b2c3d4e5f6g7h
a1b2c3d4e5f6
a1b2-4e5f6g7h
A1B2C3D4E5F6G7H
matched
a1b2c3d4e5f6g7h|q
a1b2c3d4e5f6g7h q
same
### end

### bashbox_marker_bytes_a_byte_used_as_ifs_splits_and_an_escaped_one_in_a_pattern_ma
# a byte used as IFS splits, and an escaped one in a pattern matches itself
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
v=$(printf 'a\001b\002c'); show() { printf '%s' "$1" | tr '\001\002' '12'; echo; }
IFS=$(printf '\001'); for w in $v; do show "($w)"; done
unset IFS; w=$(printf 'x\001 y'); for q in $w; do show "{$q}"; done; : ${u:=$v}; show "$u"; k=$(printf '\004'); echo "${k:+set}" "${#k}"
set -- "$k"; for p in "$@"; do show "[$p]"; done; set --; for p in "$@" "$k"; do show "<$p>"; done
b=$(printf '\002'); [[ "x${b}y" == x?y ]] && echo one-char; c="$b*"; [[ "${b}zz" == $c ]] && echo glob
### expect
(a)
(b2c)
{x1}
{y}
a1b2c
set 1
[]
<>
one-char
glob
### end

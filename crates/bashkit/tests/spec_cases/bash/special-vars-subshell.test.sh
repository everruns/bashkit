### arg0_unchanged_in_function
f() { echo "fn=${FUNCNAME[0]} same=$([[ $0 == "$top" ]] && echo y)"; }
top=$0
f
### expect
fn=f same=y
### end

### dollar_dash_flags
# `bash -c` adds `c`; drop it so script and -c runs agree
echo "${-//c/}"
set -eu; echo "${-//c/}"; set +eu
### expect
hB
ehuB
### end

### bash_subshell_levels
echo "$BASH_SUBSHELL $(echo $BASH_SUBSHELL) $( (echo $BASH_SUBSHELL) )"
( echo "p $BASH_SUBSHELL" )
echo "s $BASH_SUBSHELL" | cat
{ echo "g $BASH_SUBSHELL"; } | cat
( echo "ps $BASH_SUBSHELL" ) | cat
echo "end $BASH_SUBSHELL"
### expect
0 1 2
p 1
s 0
g 1
ps 1
end 0
### end

### read_array_keeps_empty_fields
for s in "," "a,," ",a" "a,b,,d,"; do IFS=, read -ra p <<< "$s"; echo "${#p[@]} $(printf '[%s]' "${p[@]}")"; done
### expect
1 []
2 [a][]
2 [][a]
4 [a][b][][d]
### end

### read_last_var_trailing_delimiter
IFS=, read x y <<< "a,b,"; echo "[$y]"
IFS=, read x y <<< "a,b,,"; echo "[$y]"
IFS=, read x y <<< "a,b,c,"; echo "[$y]"
### expect
[b]
[b,,]
[b,c,]
### end

### positional_slices
set -- a b c d
echo "${@:1}" "|" "${@:2}" "|" "${*:1:2}" "|" "${@: -1}" "|" "${@: -2:1}" "|" "${@:9}" "|"
printf '<%s>' "${@:2:2}"; echo
printf '<%s>' "${*:2}"; echo
echo "${#}" "${?:-unset}"
### expect
a b c d | b c d | a b | d | c | |
<b><c>
<b c d>
4 0
### end

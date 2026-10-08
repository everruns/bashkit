# Oils spec gaps closed together: quoted-segment words, subscripts with
# blanks, array literal syntax errors, array expansion and slicing,
# indirect expansion through subscripts, DEBUG traps and more.
# Each expectation was checked against bash 5.2.

### single_quote_then_double_quote_expands
n() { echo "$#:$(printf '[%s]' "$@")"; }
x=1 y=
echo 'a'"$x" 'a'"$x"c "$x"c "a"'b'"$x"c "$x"'c'
n ''"$y"
n "$y"'' ''"$y"
set -- a 'b c'
n "$@"x 'q'"$@" "$@"'' "${@}"z
n "$x"* "$x"{a,b} "$x"c'd'"$x"
### expect
a1 a1c 1c ab1c 1c
1:[]
2:[][]
8:[a][b cx][qa][b c][a][b c][a][b cz]
4:[1*][1a][1b][1cd1]
### end

### subscript_with_blanks_in_assignment
a[1 * 1]=x
a[ 1 + 2 ]=z
echo status=$? "${!a[@]}" "${a[@]}"
i=(0 1 2)
b[ i[1]+i[2] ]=3
echo "${!b[@]}"
c[1 + 2]= true
printf "%s\n" a[3 + 4]=
### expect
status=0 1 3 x z
3
a[3
+
4]=
### end

### space_before_array_paren_is_syntax_error
eval "a= (1 '2 3')"
echo "status=$?"
### expect
status=2
### end

### operator_in_array_literal_is_syntax_error
eval "a=(
1
&
'2 3'
)"
echo "status=$?"
### expect
status=2
### end

### unquoted_array_elements_split
a=(1 '2 3')
printf '[%s]' ${a[@]} ${a[*]}; echo
### expect
[1][2][3][1][2][3]
### end

### array_slice_fields_by_index
a=(abc def)
echo "${a[1]:1}" "${a[0]:0:2}"
c=(1 2 3 4 5)
printf '[%s]' "${c[@]:1:2}" "${c[@]:(-4)}"; echo
(( d[33]=1 )); (( d[66]=2 )); (( d[99]=3 ))
printf '[%s]' "${d[@]:15:2}" "${d[@]: -40}"; echo
IFS=-; echo "${c[*]:3}"
### expect
ef ab
[2][3][2][3][4][5]
[1][2][2][3]
4-5
### end

### negative_index_before_start_reads_empty
a=(x)
echo "[${a[-2]}]" $?
echo "[$((a[-2]))]" $?
b=(1)
unset -v 'b[-2]'
echo unset=$? ${#b[@]}
### expect
[] 0
[0] 0
unset=1 1
### end

### indirect_through_subscripts
f() { printf '[%s]' "${!1}"; echo; }
array=(x y z)
f 'array[0]'
f 'array[1+1]'
f 'array[@]'
f 'array[*]'
set -- p q
ref=@; printf '[%s]' "${!ref}"; echo
foo=bar; a=('1 2' foo); echo "${!a[1]}"
declare -A A=([K]=val); r='A["K"]'; echo "${!r}"
### expect
[x]
[z]
[x][y][z]
[x y z]
[p][q]
bar
val
### end

### indirect_with_operators
x=hello; r=x
echo "${!r:1:3}" "${!r#h}" "${!r/l/L}" "${!r^}"
declare -A ref=([d]='a3[@]')
a3=(1 2 3)
printf '[%s]' "${!ref[@]:1}" "${!ref[@]#1}"; echo
ar='a[@]'; a=('' '')
printf '[%s]' "${!ar:-set}"; echo
### expect
ell ello heLlo Hello
[2][3][][2][3]
[][]
### end

### indirect_bad_names_abort_the_line
a='bad var name'
echo ref ${!a}
echo status=$?
echo ${!undef}
echo status=$?
x=(ale bean); ale=zzz
echo first=${!x}
### expect
status=1
status=1
first=zzz
### end

### quoted_array_default_keeps_elements
a2=('' x); empty=()
printf '[%s]' "${a2[@]-minus}" "${empty[@]+plus}" "${empty[@]-minus}"; echo
a3=(1 2); printf '[%s]' "${a3[@]@a}"; echo
### expect
[][x][minus]
[a][a]
### end

### debug_trap_lineno_loops_and_case
trap 'echo "dbg $LINENO"' DEBUG
x=1
for i in a b; do :; done
case $x in 1) echo one;; esac
trap - DEBUG
echo done
### expect
dbg 2
dbg 3
dbg 3
dbg 3
dbg 3
dbg 4
dbg 4
one
dbg 5
done
### end

### debug_trap_scopes_and_functrace
f() {
  echo in-f
}
trap 'echo dbg' DEBUG
f
( echo sub )
v=$(echo cs); trap - DEBUG
echo "v=$v"
set -T
trap 'echo "T $LINENO"' DEBUG
f
trap - DEBUG
### expect
dbg
in-f
sub
dbg
dbg
v=cs
T 11
T 1
T 2
in-f
T 12
### end

### debug_trap_pipeline_runs_in_parent
trap 'echo d' DEBUG
echo a | cat
trap - DEBUG
### expect
d
d
a
d
### end

### debug_trap_return_and_exit
f() { trap 'return 7' DEBUG; echo unreached; }
f; echo "f=$?"
trap - DEBUG
( trap 'exit 3' DEBUG; echo no ); echo "sub=$?"
### expect
f=7
sub=3
### end

### trap_handler_with_blanks_and_printf_zero_flag
trap ' 42 ' EXIT; echo st=$?
trap - EXIT
printf '[%06s][%0.0s][%05d]\n' ab cd 7
### expect
st=0
[    ab][][00007]
### end

### arith_in_subscripts
a=(10 20 30 40 50 60)
a[5&3]=x; echo "${a[1]}"
i=1; echo $(( a[$((i+1))] + $((2*$((3)))) ))
b=(); b[i++]=v; b[i++]=w; echo "${b[@]} i=$i"
### expect
x
36
v w i=3
### end

### list_to_array_member_aborts_line
a=(1 2)
a[0]=(3 4); echo after
echo "status=$? ${a[*]}"
### expect
status=1 1 2
### end

### nested_expansion_in_slices
set -- x 2
a=(1 2 3 4)
echo "${a[@]:$((${2:-1}))}"
s=abcdef
echo "${s: 0 < 1 ? 2 : 0 : 1}"
echo "${a[@]:1:2}" "${a[*]: -1}"
### expect
3 4
c
2 3 4
### end

### indirect_default_word_splits
pkgs=(p q); name=pkgs
for e in ${!name+"${!name}"}; do echo "e=$e"; done
v=hello; n=v; echo "${!n^}" "${!n:1:2}" "${!n/l/L}"
echo "[${empty[*]}]"
### expect
e=p
Hello el heLlo
[]
### end

### fatal_assignment_error_ends_only_the_subshell
(X=${x?bc}) 2>/dev/null
echo "sub=$?"
v=$(X=${x?bc}) 2>/dev/null
echo "cmdsub=$?"
### expect
sub=1
cmdsub=1
### end

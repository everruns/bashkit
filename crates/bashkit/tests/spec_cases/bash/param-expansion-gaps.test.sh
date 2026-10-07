### toggle_case
s="hello World"
echo "${s~~}|${s~}"
a=(abC Def); echo "${a[@]~~}"
### expect
HELLO wORLD|Hello World
ABc dEF
### end

### replace_escaped_slash
p="a.b.c"
echo "${p//./\/}" ${p//./\/} "${p/./\/}"
### expect
a/b/c a/b/c a/b.c
### end

### prefix_names_split_in_for
cfg_host=localhost cfg_port=80 other=1
for v in ${!cfg_@}; do echo "$v=${!v}"; done
n=0; for v in ${!cfg_*}; do n=$((n+1)); done; echo $n
set -- ${!cfg_@}; echo $#
### expect
cfg_host=localhost
cfg_port=80
2
2
### end

### mapfile_delim
mapfile -t -d , campos <<< "a,b,c"
printf '[%s]' "${campos[@]}"; echo
mapfile -d , keep <<< "x,y"
printf '<%s>' "${keep[@]}"; echo
### expect
[a][b][c
]
<x,><y
>
### end

### mapfile_skip_count_origin
printf 'l0\nl1\nl2\nl3\n' > /tmp/mf.txt
readarray -t -s 1 -n 2 parte < /tmp/mf.txt
echo "${parte[*]}"
arr=(keep)
mapfile -t -O 2 arr < /tmp/mf.txt
declare -p arr
mapfile -tn1 one < /tmp/mf.txt; echo "${#one[@]} ${one[0]}"
### expect
l1 l2
declare -a arr=([0]="keep" [2]="l0" [3]="l1" [4]="l2" [5]="l3")
1 l0
### end

### mapfile_nul_delim
printf 'a\0b\0' | { mapfile -d '' arr; echo ${#arr[@]}; }
### expect
2
### end

### mapfile_bad_option
mapfile -x arr; echo "rc=$?"
### expect
rc=2
### end

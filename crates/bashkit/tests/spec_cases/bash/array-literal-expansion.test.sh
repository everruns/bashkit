### array_literal_glob
cd /tmp && mkdir -p alg && cd alg && touch b.txt a.txt c.log
arr=(*.txt)
echo "${#arr[@]} ${arr[*]}"
### expect
2 a.txt b.txt
### end

### array_literal_brace
arr=(x{1,2} {a..c})
echo "${#arr[@]} ${arr[*]}"
### expect
5 x1 x2 a b c
### end

### array_literal_quoted_kept
cd /tmp && mkdir -p alq && cd alq && touch a.txt
arr=("*.txt" '{1,2}' "x"*.txt)
printf '<%s>' "${arr[@]}"; echo
### expect
<*.txt><{1,2}><x*.txt>
### end

### array_literal_nullglob
cd /tmp && mkdir -p aln && cd aln
shopt -s nullglob
arr=(*.zz); a=(x*.zz y); b=("*.zz")
echo "${#arr[@]} ${#a[@]} ${#b[@]}"
### expect
0 1 1
### end

### array_literal_unquoted_var_globs
cd /tmp && mkdir -p alv && cd alv && touch p1 p2
pat='p*'
arr=($pat)
echo "${arr[*]}"
### expect
p1 p2
### end

### array_literal_indexed_unchanged
arr=([2]=x y [0]=z)
declare -p arr
### expect
declare -a arr=([0]="z" [2]="x" [3]="y")
### end

### command_v_multiple_names
command -v echo cd
echo "rc=$?"
command -v nope_xyz echo
echo "rc=$?"
command -v nope_xyz nope2_xyz
echo "rc=$?"
### expect
echo
cd
rc=0
echo
rc=0
rc=1
### end

### array_concat_at_expansion
# "${arr[@]}" in array literal should splat individual elements
a=(x y z)
b=(1 2)
c=("${a[@]}" "${b[@]}")
echo "${#c[@]}"
echo "${c[2]}"
### expect
5
z
### end

### array_splat_single_source
# Single "${arr[@]}" in array context
orig=(a b c d)
copy=("${orig[@]}")
echo "${#copy[@]}"
echo "${copy[3]}"
### expect
4
d
### end

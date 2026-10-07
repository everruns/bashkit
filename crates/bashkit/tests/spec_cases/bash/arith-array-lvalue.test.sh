# Arithmetic assignment to array elements: ((a[i]++)), ((c[k]+=n)), let a[i]=v

### arr_postinc_indexed
a=(1 2); ((a[1]++)); echo "${a[1]}"
### expect
3
### end

### arr_postinc_assoc_bare_key
declare -A c; ((c[x]++)); ((c[x]++)); echo "${c[x]}"
### expect
2
### end

### arr_compound_assoc_var_key
declare -A c; k=foo; ((c[$k]+=3)); echo "${c[foo]}"
### expect
3
### end

### arr_assign_new_index
a=(1 3); ((a[2]=5)); echo "${a[*]}"
### expect
1 3 5
### end

### arr_let_postinc
a=(1 3); let a[1]++; echo "${a[1]}"
### expect
4
### end

### arr_var_index_compound
a=(1 2); i=0; ((a[i]+=5)); echo "${a[0]}"
### expect
6
### end

### arr_quoted_assoc_key
declare -A c; ((c["x y"]++)); echo "${c[x y]}"
### expect
1
### end

### arr_expansion_inc_value
a=(0 4); echo $((a[1]++)) $((++a[1])) ${a[1]}
### expect
4 6 6
### end

### arr_assign_creates_array
((b[3]=7)); declare -p b
### expect
declare -a b=([3]="7")
### end

### arr_index_expression
a=(1 6); i=0; ((a[i+1]*=2)); echo "${a[1]}"
### expect
12
### end

### arr_predec
a=(6 1); ((--a[0])); echo "${a[0]}"
### expect
5
### end

### arr_assoc_postdec_value
declare -A c; c[x]=4; echo $((c[x]--)) ${c[x]}
### expect
4 3
### end

### arr_negative_index
a=(5 12 5); ((a[-1]+=1)); echo "${a[*]}"
### expect
5 12 6
### end

### arr_assoc_key_embedded_var
declare -A c; for ((i = 0; i < 3; i++)); do ((c[k$i]+=1)); done; ((c[${i}x]++)); echo "${!c[@]}" | tr ' ' '\n' | sort | tr '\n' ' '; echo
### expect
3x k0 k1 k2 
### end

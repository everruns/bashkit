### read_basic
### bash_diff
echo "hello" | read var; echo "$var"
### expect
hello
### end

### read_multiple_vars
echo "one two three" | { read a b c; echo "$a $b $c"; }
### expect
one two three
### end

### read_ifs
echo "a:b:c" | { IFS=: read x y z; echo "$x $y $z"; }
### expect
a b c
### end

### read_herestring
read var <<< "from herestring"
echo "$var"
### expect
from herestring
### end

### read_empty_input
echo "" | { read var; echo ">${var}<"; }
### expect
><
### end

### read_r_flag
read -r var <<< "hello\nworld"
echo "$var"
### expect
hello\nworld
### end

### read_leftover
echo "one two three four" | { read a b; echo "$a|$b"; }
### expect
one|two three four
### end

### read_array
# read -a reads words into indexed array
read -a arr <<< "one two three"
echo "${arr[0]} ${arr[1]} ${arr[2]}"
### expect
one two three
### end

### read_array_length
# read -a array length
read -a arr <<< "a b c d"
echo ${#arr[@]}
### expect
4
### end

### read_ifs_empty_fields
# IFS=: with consecutive delimiters preserves empty fields
IFS=: read a b c d <<< "one::three:"
echo "a=$a b=$b c=$c d=$d"
### expect
a=one b= c=three d=
### end

### read_ifs_empty_fields_leading
# Leading delimiter produces empty first field
IFS=: read a b c <<< ":two:three"
echo "a=$a b=$b c=$c"
### expect
a= b=two c=three
### end

### read_nchars
# read -n N reads N characters
read -n 3 var <<< "hello"
echo "$var"
### expect
hel
### end

### read_eof_clears_variable
# read at EOF with no data should clear the variable
printf "one\ntwo" | {
  count=0
  while IFS= read -r line || [[ -n "$line" ]]; do
    echo "$line"
    count=$((count + 1))
    [[ $count -gt 5 ]] && break
  done
}
### expect
one
two
### end

### read_eof_partial_line
# read returns 1 but captures partial line without trailing newline
printf "complete\npartial" | {
  lines=()
  while IFS= read -r line || [[ -n "$line" ]]; do
    lines+=("$line")
    [[ ${#lines[@]} -gt 5 ]] && break
  done
  printf '%s\n' "${lines[@]}"
}
### expect
complete
partial
### end

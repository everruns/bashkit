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

### read_nchars
# read -n N reads N characters
read -n 3 var <<< "hello"
echo "$var"
### expect
hel
### end

### dbracket_string_order
[[ a < B ]]; echo "a<B:$?"
[[ B < a ]]; echo "B<a:$?"
[[ a > B ]]; echo "a>B:$?"
[[ 9 < 10 ]]; echo "9<10:$?"
[[ "" < B ]]; echo "empty:$?"
### expect
a<B:1
B<a:0
a>B:0
9<10:1
empty:0
### end

### dbracket_string_order_with_expansions
x=apple; y=Banana
[[ $x < $y ]]; echo "$?"
[[ "$x" > "$y" ]] && echo gt
[[ $x < $y || $x > $y ]] && echo ordered
### expect
1
gt
ordered
### end

### shopt_listing_width
shopt extglob nullglob | cat -A
shopt -s extglob
shopt extglob
shopt -u extglob
### expect
extglob        ^Ioff$
nullglob       ^Ioff$
extglob        	on
### end

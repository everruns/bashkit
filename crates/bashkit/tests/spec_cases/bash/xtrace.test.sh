# set -x trace text, compared against bash 5.2

### xtrace_word_quoting
# set -x quotes traced words the way bash re-reads them
{ set -x; echo 'a b' '' "it's" '*' a# '#a' '~x' x~ "'" $'\001' 'a=~b'; set +x; } 2>&1 >/dev/null
### expect
+ echo 'a b' '' 'it'\''s' '*' a# '#a' '~x' x~ \' $'\001' 'a=~b'
+ set +x
### end

### xtrace_assignments
# assignments are traced; arrays as written, before expansion
{ set -x; a=1; b="x y"; c=; arr=(1 'two 2' $(echo 3)); arr[3]=z; a+=2; set +x; } 2>&1
### expect
+ a=1
+ b='x y'
+ c=
+ arr=(1 'two 2' $(echo 3))
++ echo 3
+ arr[3]=z
+ a+=2
+ set +x
### end

### xtrace_prefix_assignment_and_redirect
# prefix assignments trace first; the trace precedes the command's redirects
{ set -x; V=1 echo hi 2>/dev/null; set +x; } 2>&1 >/dev/null
### expect
+ V=1
+ echo hi
+ set +x
### end

### xtrace_ps4_expansion_and_nesting
# PS4 is expanded; $(...) and eval repeat its first character
f() { echo "in $1" >/dev/null; }
{ PS4='+$((1+1)) '; set -x; x=$(echo sub); eval 'echo ev' >/dev/null; f 'a b'; set +x; } 2>&1
### expect
++2 echo sub
+2 x=sub
+2 eval 'echo ev'
++2 echo ev
+2 f 'a b'
+2 echo 'in a b'
+2 set +x
### end

### xtrace_empty_ps4
# an empty PS4 traces without a prefix
{ PS4=''; set -x; echo quiet >/dev/null; set +x; } 2>&1
### expect
echo quiet
set +x
### end

### xtrace_conditional_terms
# [[ ]] traces each primary it evaluates
{ set -x; [[ abc == a* && -n "" ]]; [[ ! -z x || y ]]; [[ $(echo hi) == hi ]]; set +x; } 2>&1
### expect
+ [[ abc == a* ]]
+ [[ -n '' ]]
+ [[ ! -z x ]]
++ echo hi
+ [[ hi == hi ]]
+ set +x
### end

### cond_negation_binds_tighter_than_or
# ! applies to one primary, not the whole || list
[[ ! -z x || y ]]; echo $?
[[ ! -n x && y ]]; echo $?
[[ ! ( -z x || y ) ]]; echo $?
### expect
0
1
1
### end

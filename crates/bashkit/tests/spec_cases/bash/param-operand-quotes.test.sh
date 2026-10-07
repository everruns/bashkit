### default_single_quotes_removed_unquoted
unset x
echo ${x:-'d'} ${x-'d e'} ${x:-a'b c'd}
### expect
d d e ab cd
### end

### default_single_quotes_kept_in_double_quotes
unset x
echo "${x:-'d'}" "${x:+'s'}"
### expect
'd' 
### end

### default_single_quotes_are_literal
unset x; y=Y
echo ${x:-'$y'} "${x:-'$y'}" ${x:-"$y"}
### expect
$y 'Y' Y
### end

### default_quoted_brace_does_not_close
unset x
echo ${x:-'}'} "${x:-'}'}" ${x:-'a}b'} ${x:-"}"}
### expect
} '}' a}b }
### end

### default_escaped_brace
unset x
echo ${x:-a\}b}
### expect
a}b
### end

### replacement_and_assign_single_quotes
x=v; unset z
echo ${x:+'r'} ${z='as'} "$z"
### expect
r as as
### end

### default_ansi_c_operand
unset x
printf '<%s>' ${x:-$'a\x41b'} ${x:-$'}'}; echo
### expect
<aAb><}>
### end

### replace_pattern_expands_variable
x=a.a; p=.
echo ${x/$p/-} "${x//$p/-}"
### expect
a-a a-a
### end

### replace_pattern_quoted_and_escaped
x='a*b.c'
echo ${x/'*'/-} ${x/"*"/-} ${x/\*/-} "${x/'.'/+}" ${x/\./+}
### expect
a-b.c a-b.c a-b.c a*b+c a*b+c
### end

### replace_pattern_quoted_slash
x=a/b/c
echo ${x/'/'/-} "${x//"/"/-}"
### expect
a-b/c a-b-c
### end

### replace_glob_leftmost_longest
x=xaYaZ
echo ${x/a*/-} ${x/a?/-} ${x//a?/-} ${x/[aY]/-} ${x//[!a]/.}
### expect
x- x-aZ x-- x-YaZ .a.a.
### end

### replace_glob_star_middle
x=one.two.three
echo ${x/.*./:} ${x//t?/T} ${x/#o*e/X} ${x/%t*e/X}
### expect
one:three one.To.Tree X one.X
### end

### replace_anchored_with_variable
x=prefix-body-suffix; p=prefix; s=suffix
echo ${x/#$p/P} ${x/%$s/S} ${x/#body/B}
### expect
P-body-suffix prefix-body-S prefix-body-suffix
### end

### replace_empty_and_star
x=abc
echo "${x//*/-}" "${x/b*/-}" "${x/z*/-}"
### expect
- a- abc
### end

### remove_pattern_single_quotes_in_double_quotes
x=a.b.c
echo "${x#'a.'}" "${x%'.c'}" ${x##'a.'} "${x%%'.'*}"
### expect
b.c a.b b.c a
### end

### assoc_subscript_single_quotes
declare -A a; a[k]=v
echo ${a['k']} "${a['k']}" ${a['k']:-none}
### expect
v v v
### end

### ansi_c_pattern_in_double_quotes
s=$'it\'s'
echo "${s%$'\'s'}" ${s/$'\''/_}
### expect
it it_s
### end

### replace_extglob_and_empty_matches
shopt -s extglob
x=xaab; y=abc; e=
echo "${x/+(a)/-} ${x//?(a)/-} ${x/?(a)/-} ${x//*(z)/-} ${y//*(b)/-}"
echo "[${e//*/-}] [${e/#/-}] [${e/*/-}]"
### expect
x-b -x---b -xaab -x-a-a-b -a--c
[-] [-] [-]
### end

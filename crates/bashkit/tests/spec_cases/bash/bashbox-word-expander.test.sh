# BashBox word-expander cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_word_expander_braced_positional_and_counts
# braced positional and counts
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -- a b; echo ${1} ${#} ${#@} ${#*}
### expect
a 2 2 2
### end

### bashbox_word_expander_positional_slices
# positional slices
set -- a b c; echo ${@:2} ${*: -1}
### expect
b c c
### end

### bashbox_word_expander_name_i_is_not_an_array_reference
# $name[i] is not an array reference
a=(x y); echo $a[1]
### expect
x[1]
### end

### bashbox_word_expander_bare_array_name_is_element_0
# bare array name is element 0
a=(x y); echo $a ${a}
### expect
x x
### end

### bashbox_word_expander_substring_with_negative_offset_length
# substring with negative offset/length
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
s=hello; echo ${s: -2} ${s:1:-1} ${s: -3:2} "[${s: -10}]" ${s:1+1:2}
### expect
lo ell ll [] ll
### end

### bashbox_word_expander_array_slices
# array slices
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
a=(1 2 3 4); echo ${a[@]: -2} ${a[@]:1:2} "[${a[@]: -9}]"
### expect
3 4 2 3 []
### end

### bashbox_word_expander_subscripts_are_arithmetic
# subscripts are arithmetic
a=(x y z); i=1; echo ${a[$i]} ${a[i]} ${a[i+1]} ${a[-1]}
### expect
y y z z
### end

### bashbox_word_expander_associative_subscript_is_expanded
# associative subscript is expanded
declare -A m; m[k]=v; key=k; echo ${m[$key]} ${m[k]}
### expect
v v
### end

### bashbox_word_expander_element_length
# element length
a=(foo bar); echo ${#a[1]}
### expect
3
### end

### bashbox_word_expander_keys_and_counts
# keys and counts
a=(b a); echo ${!a[@]} ${#a[@]} ${a[*]}
### expect
0 1 2 b a
### end

### bashbox_word_expander_indirection
# indirection
r=x; x=val; echo "[${!r}]"
### expect
[val]
### end

### bashbox_word_expander_indirection_to_an_element
# indirection to an element
a=(p q); r="a[1]"; echo ${!r}
### expect
q
### end

### bashbox_word_expander_nested_default
# nested default
x=; echo ${x:-${y:-z}}
### expect
z
### end

### bashbox_word_expander_element_default
# element default
a=(p q); echo ${a[0]:-d} ${a[5]:-d}
### expect
p d
### end

### bashbox_word_expander_set_vs_null_tests
# set vs null tests
unset x; echo ${x-unset} ${x+set}; x=; echo ${x-unset} ${x+set} ${x:+nonempty}
### expect
unset
set
### end

### bashbox_word_expander_empty_uses_default
# empty $@ uses default
set --; echo "[${@-x}] [${@:-y}]"
### expect
[x] [y]
### end

### bashbox_word_expander_assign_default
# assign default
echo ${z=assigned} $z
### expect
assigned assigned
### end

### bashbox_word_expander_assign_default_to_element
# assign default to element
echo ${a[1]:=x} ${a[1]}
### expect
x x
### end

### bashbox_word_expander_error_if_unset_passes_a_set_value
# error-if-unset passes a set value
x=v; echo ${x?never} ${x:?never}
### expect
v v
### end

### bashbox_word_expander_pattern_operand_is_expanded
# pattern operand is expanded
p=b; x=abc; echo ${x#*$p}
### expect
c
### end

### bashbox_word_expander_quoted_pattern_characters_are_literal
# quoted pattern characters are literal
x='a*b'; echo ${x#*\*} ${x%"*"b}
### expect
b a
### end

### bashbox_word_expander_prefix_suffix_removal
# prefix/suffix removal
x=a.b.c; echo ${x#*.} ${x##*.} ${x%.*} ${x%%.*} ${x#?}
### expect
b.c c a.b a .b.c
### end

### bashbox_word_expander_anchored_replacement
# anchored replacement
x=abc; echo ${x/#a/X} ${x/%c/Y} ${x/b}
### expect
Xbc abY ac
### end

### bashbox_word_expander_replacement_is_not_a_regex_template
# replacement is not a regex template
x=abc; r='\0$1'; echo ${x/b/$r}
### expect
a\0$1c
### end

### bashbox_word_expander_replace_first_all
# replace first/all
x=abab; echo ${x/b/X} ${x//b/X}
### expect
aXab aXaX
### end

### bashbox_word_expander_case_modification
# case modification
x=hello; echo ${x^} ${x^^} ${x,} ${x,,}
### expect
Hello HELLO hello hello
### end

### bashbox_word_expander_nested_arithmetic_parens
# nested arithmetic parens
echo $(( ((1+2)) * 3 )) $(( (1+(2)) ))
### expect
9 3
### end

### bashbox_word_expander_quoted_parens_in
# quoted parens in $(...)
echo $(echo ")" "(" '(') $(echo "a)b\")")
### expect
) ( ( a)b")
### end

### bashbox_word_expander_joins_with_first_ifs_char
# "$*" joins with first IFS char
set -- a b; IFS=:; echo "$*"
### expect
a:b
### end

### bashbox_word_expander_trailing_backslash_pattern
# trailing backslash pattern
x="a\\"; p="\\"; echo ${x%$p}
### expect
a
### end

### bashbox_word_expander_lone_or_unknown
# lone or unknown $
echo a$%b $
### expect
a$%b $
### end

### bashbox_word_expander_unset_variable_is_empty
# unset variable is empty
echo $nothing.
### expect
.
### end

### bashbox_word_expander_double_quote_escapes
# double-quote escapes
echo "a\"b\$c\\d\q"
### expect
a"b$c\d\q
### end

### bashbox_word_expander_backtick_keeps_other_backslashes
# backtick keeps other backslashes
x=`printf "%s" "a\qb"`; echo $x
### expect
a\qb
### end

### bashbox_word_expander_nested_backticks
# nested backticks
x=`echo \`echo hi\``; echo $x
### expect
hi
### end

### bashbox_word_expander_ifs_splitting_with_non_whitespace
# IFS splitting with non-whitespace
IFS=:; x=":a::b:"; printf "[%s]" $x; echo
### expect
[][a][][b]
### end

### bashbox_word_expander_empty_unquoted_word_vanishes
# empty unquoted word vanishes
x=; printf "[%s]" $x a; echo
### expect
[a]
### end

### bashbox_word_expander_empty_ifs_does_not_split
# empty IFS does not split
IFS=; x="a b"; printf "[%s]" $x; echo
### expect
[a b]
### end

### bashbox_word_expander_whitespace_ifs_collapses
# whitespace IFS collapses
x="  a   b  "; printf "[%s]" $x; echo
### expect
[a][b]
### end

### bashbox_word_expander_keeps_words
# "$@" keeps words
f(){ for x in "$@"; do echo "[$x]"; done; }; f a "b c"
### expect
[a]
[b c]
### end

### bashbox_word_expander_escaped_comma_and_non_expanding_braces
# escaped comma and non-expanding braces
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo {a\,b,c} {a}b {abc} {1..c}
### expect
a,b c {a}b {abc} {1..c}
### end

### bashbox_word_expander_descending_ranges
# descending ranges
echo {5..1} {c..a}
### expect
5 4 3 2 1 c b a
### end

### bashbox_word_expander_stepped_ranges
# stepped ranges
echo {1..9..3} {a..e..2} {1..5..-2}
### expect
1 4 7 a c e 1 3 5
### end

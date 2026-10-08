# Word splitting tests
# Inspired by Oils spec/word-split.test.sh
# https://github.com/oilshell/oil/blob/master/spec/word-split.test.sh

### ws_ifs_scoped
# IFS is scoped with local
### skip: TODO local IFS not checked during word splitting
IFS=b
word=abcd
f() { local IFS=c; echo "$word" | tr "$IFS" '\n' | while read part; do printf '[%s]' "$part"; done; echo; }
# Actually test splitting directly
f2() { local IFS=c; set -- $word; echo "$#:$1:$2"; }
f2
IFS=b
set -- $word
echo "$#:$1:$2"
### expect
2:a:d
2:a:cd
### end

### ws_tilde_not_split
# Tilde sub is not split, but var sub is
HOME="foo bar"
set -- ~
echo $#
set -- $HOME
echo $#
### expect
1
2
### end

### ws_word_joining
# Word splitting with quoted and unquoted parts
### skip: TODO quoted/unquoted word joining at split boundaries not implemented
a="1 2"
b="3 4"
set -- $a"$b"
echo $#
echo "$1"
echo "$2"
### expect
2
1
23 4
### end

### ws_word_joining_complex
# Complex word splitting with multiple parts
### skip: TODO quoted/unquoted word joining at split boundaries not implemented
a="1 2"
b="3 4"
c="5 6"
d="7 8"
set -- $a"$b"$c"$d"
echo "$#"
echo "$1"
echo "$2"
echo "$3"
### expect
3
1
23 45
67 8
### end

### ws_dollar_star
# $* splits arguments
fun() { set -- $*; echo "$#:$1:$2:$3"; }
fun "a 1" "b 2"
### expect
4:a:1:b
### end

### ws_quoted_dollar_star
# "$*" joins with first char of IFS
fun() { echo "$*"; }
fun "a 1" "b 2" "c 3"
### expect
a 1 b 2 c 3
### end

### ws_dollar_at
# $@ splits arguments
fun() { set -- $@; echo "$#:$1:$2:$3"; }
fun "a 1" "b 2"
### expect
4:a:1:b
### end

### ws_quoted_dollar_at
# "$@" preserves arguments
fun() { echo $#; for a in "$@"; do echo "[$a]"; done; }
fun "a 1" "b 2" "c 3"
### expect
3
[a 1]
[b 2]
[c 3]
### end

### ws_empty_argv
# empty $@ and $* are elided
set --
set -- 1 "$@" 2 $@ 3 "$*" 4 $* 5
echo "$#"
echo "$1 $2 $3 $4 $5"
### expect
6
1 2 3  4
### end

### ws_star_empty_ifs
# $* with empty IFS
set -- "1 2" "3  4"
IFS=
set -- $*
echo $#
echo "$1"
echo "$2"
### expect
2
1 2
3  4
### end

### ws_star_empty_ifs_quoted
# "$*" with empty IFS joins without separator
set -- "1 2" "3  4"
IFS=
echo "$*"
### expect
1 23  4
### end

### ws_elision_space
# Unquoted whitespace-only var is elided
s1=' '
set -- $s1
echo $#
### expect
0
### end

### ws_elision_nonwhitespace_ifs
# Non-whitespace IFS char produces empty field
### skip: TODO non-IFS whitespace in $space not elided correctly with custom IFS
IFS='_'
char='_'
space=' '
empty=''
set -- $char; echo $#
set -- $space; echo "$1"
set -- $empty; echo $#
### expect
1

0
### end

### ws_leading_trailing_nonwhitespace_ifs
# Leading/trailing with non-whitespace IFS
IFS=_
s1='_a_b_'
set -- $s1
echo "$#:$1:$2:$3"
### expect
3::a:b
### end

### ws_mixed_ifs_whitespace_nonwhitespace
# Mixed whitespace and non-whitespace IFS
IFS='_ '
s1='_ a  b _ '
s2='  a  b _ '
set -- $s1; echo "$#:$1:$2:$3"
set -- $s2; echo "$#:$1:$2"
### expect
3::a:b
2:a:b
### end

### ws_multiple_nonwhitespace_ifs
# Multiple non-whitespace IFS chars produce empty fields
IFS=_-
s1='a__b---c_d'
set -- $s1
echo "$#"
for arg in "$@"; do echo "[$arg]"; done
### expect
7
[a]
[]
[b]
[]
[]
[c]
[d]
### end

### ws_ifs_whitespace_and_nonwhitespace
# IFS with whitespace and non-whitespace
IFS='_ '
s1='a_b _ _ _ c  _d e'
set -- $s1
echo "$#"
for arg in "$@"; do echo "[$arg]"; done
### expect
7
[a]
[b]
[]
[]
[c]
[d]
[e]
### end

### ws_empty_at_star_elided
# empty $@ and $* are elided in argument list
fun() { set -- 1 $@ $* 2; echo $#; }
fun
### expect
2
### end

### ws_unquoted_empty_elided
# unquoted empty var is elided
empty=""
set -- 1 $empty 2
echo $#
### expect
2
### end

### ws_unquoted_whitespace_elided
# unquoted whitespace var is elided
space=" "
set -- 1 $space 2
echo $#
### expect
2
### end

### ws_empty_literal_not_elided
# empty literal prevents elision
### skip: TODO word elision not implemented
space=" "
set -- 1 $space"" 2
echo $#
### expect
3
### end

### ws_no_split_empty_ifs
# no splitting when IFS is empty
IFS=""
foo="a b"
set -- $foo
echo "$#:$1"
### expect
1:a b
### end

### ws_default_value_multiple_words
# default value can yield multiple words
### skip: TODO word splitting in default values not implemented
set -- ${undefined:-"2 3" "4 5"}
echo "$#"
echo "$1"
echo "$2"
### expect
2
2 3
4 5
### end

### ws_default_value_part_joining
# default value with part joining
### skip: TODO word splitting in default values not implemented
set -- 1${undefined:-"2 3" "4 5"}6
echo "$#"
echo "$1"
echo "$2"
### expect
2
12 3
4 56
### end

### ws_ifs_empty_no_split
# IFS empty prevents all splitting
IFS=''
x="a b	c"
set -- $x
echo "$#:$1"
### expect
1:a b	c
### end

### ws_ifs_unset_default
# IFS unset behaves like space/tab/newline
unset IFS
x="a b	c"
set -- $x
echo "$#:$1:$2:$3"
### expect
3:a:b:c
### end

### ws_ifs_backslash
# IFS=backslash splits on backslash
IFS='\'
s='a\b'
set -- $s
echo "$#:$1:$2"
### expect
2:a:b
### end

### ws_ifs_glob_metachar_star
# IFS characters that are glob metacharacters
IFS='* '
s='a*b c'
set -f
set -- $s
echo "$#:$1:$2:$3"
set +f
### expect
3:a:b:c
### end

### ws_ifs_glob_metachar_question
# IFS with ? glob metacharacter
IFS='?'
s='?x?y?z?'
set -f
set -- $s
echo "$#"
for arg in "$@"; do echo "[$arg]"; done
set +f
### expect
4
[]
[x]
[y]
[z]
### end

### ws_empty_ifs_star_join
# Empty IFS and $* join
IFS=
echo ["$*"]
set a b c
echo ["$*"]
### expect
[]
[abc]
### end

### ws_unset_ifs_star_join
# Unset IFS and $* join with space
set a b c
unset IFS
echo ["$*"]
### expect
[a b c]
### end

### ws_ifs_custom_char
# IFS=o doesn't break echo
IFS=o
echo hi
### expect
hi
### end

### ws_ifs_custom_at_join
# IFS and joining $@ vs $*
IFS=:
set -- x 'y z'
for a in "$@"; do echo "[@$a]"; done
for a in "$*"; do echo "[*$a]"; done
### expect
[@x]
[@y z]
[*x:y z]
### end

### ws_ifs_custom_at_assignment
# IFS and $@ / $* in assignments
IFS=:
set -- x 'y z'
s="$@"
echo "at=$s"
s="$*"
echo "star=$s"
### expect
at=x y z
star=x:y z
### end

### ws_ifs_empty_at_preserved
# IFS='' with $@ preserves args
set -- a 'b c'
IFS=''
set -- $@
echo "$#"
echo "$1"
echo "$2"
### expect
2
a
b c
### end

### ws_ifs_empty_array_preserved
# IFS='' with ${a[@]} preserves elements
myarray=(a 'b c')
IFS=''
set -- ${myarray[@]}
echo "$#"
echo "$1"
echo "$2"
### expect
2
a
b c
### end

### ws_unicode_in_ifs
# Unicode in IFS
### skip: TODO bash does byte-level IFS splitting for multibyte chars; bashkit uses char-level
x=çx IFS=ç
set -- $x
echo "$#"
printf "<%s>\n" "$@"
### expect
2
<>
<x>
### end

### ws_default_value_ifs_unquoted
# Default value with unquoted IFS char
### skip: TODO word splitting in default values not implemented
IFS=_
set -- ${v:-AxBxC}
echo "$#:$1"
IFS=_
set -- ${v:-A_B_C}
echo "$#"
for a in "$@"; do echo "[$a]"; done
### expect
1:AxBxC
3
[A]
[B]
[C]
### end

### ws_empty_string_both_sides
# ""$A"" - empty string on both sides
### skip: TODO quoted empty string prevents elision at word split boundaries
A="   abc   def   "
set -- ""$A""
echo "$#"
echo "[$1]"
echo "[$2]"
echo "[$3]"
echo "[$4]"
### expect
4
[]
[abc]
[def]
[]
### end

### quoted_at_inside_word_splits_per_element
# "x$@y" and x"${a[@]}"y: prefix joins the first element, suffix the last
set -- "a b" c; a=(1 2); printf '[%s]' "x$@y" x"${a[@]}"y "${a[@]}z"; echo
### expect
[xa b][cy][x1][2y][1][2z]
### end

### alternate_operand_at_keeps_fields
# ${1+"$@"} expands to the positional parameters, one field each
set -- "a b" c; printf '[%s]' "${1+"$@"}" ${1+"$@"} "${zz-$@}"; echo
### expect
[a b][c][a b][c][a b][c]
### end

### literal_ifs_char_beside_expansion_not_split
# Only expansion results split: a literal `:` next to ${w} stays text
IFS=:; w=a:; printf '[%s]' ${w}:b ${w}:; echo
IFS=z; w=az; printf '[%s]' ${w}zb; echo
### expect
[a][:b][a][:]
[a][zb]
### end

### empty_quotes_beside_expansion_keep_field
# $space"" and ""$A"" keep the empty quoted parts as fields
space=" "; A="  abc  def  "
printf '[%s]' 1 $space"" 2 $space'' 3; echo
printf '[%s]' ""$A""; echo
for i in ""$A""; do echo "=$i="; done
### expect
[1][][2][][3]
[][abc][def][]
==
=abc=
=def=
==
### end

### quoted_text_inside_unquoted_word_not_split
x=1; y="p q"; printf '[%s]' $x'a b' $y\ r; echo
### expect
[1a b][p][q r]
### end

### default_operand_quotes_protect_text
# ${u:-"2 3" "4 5"}: quoted operand text stays whole, the blank between splits
printf '[%s]' 1 ${u:-"2 3" "4 5"} 6; echo
printf '[%s]' 1${u:-"2 3" "4 5"}6; echo
IFS=_; printf '[%s]' 1${u:-"2_3"x_x"4_5"}6; echo
IFS=x; v=; echo ${v:-AxBxC}x ${v:-"AxBxC"}x
### expect
[1][2 3][4 5][6]
[12 3][4 56]
[12_3x][x4_56]
A B Cx AxBxCx
### end

### unquoted_star_nonwhitespace_ifs_keeps_empty
# Unquoted $* / $@ join on the first IFS char, then split
set -- one '' two; IFS=zx
printf '[%s]' $* ; echo
printf '[%s]' =$@=; echo
set -- '' '' '' ''; printf '<%s>' $*; echo " $#"
### expect
[one][][two]
[=one][][two=]
<><><> 4
### end

### unquoted_at_null_ifs
# A null IFS never splits, but $@ / $* still give one field per parameter
IFS=; set -- 'a b' '' c
printf '[%s]' $@; echo
printf '[%s]' =$*=; echo
x=; printf '[%s]' $x; echo "n=$#"
### expect
[a b][c]
[=a b][c=]
[]n=3
### end

### length_result_is_split
v=hello; IFS=5; echo "[${#v}]" ${#v} end
### expect
[5]  end
### end

### name_list_star_forms_under_null_ifs
a=(x y z); pa=1; pb=2; IFS=
printf '[%s]' ${!a[*]} "${!a[*]}" ${!a[@]}; echo
printf '[%s]' ${!p*} "${!p*}" ${!p@} "x${!p@}y"; echo
### expect
[0 1 2][012][0][1][2]
[papb][papb][pa][pb][xpa][pby]
### end

### expansion_after_quoted_prefix_runs_once
i=0; echo {a,b}-$((i++)) "q"$((i++)) "c"$(echo d) "u"${z:-e f}; echo $i
### expect
a-0 b-1 q2 cd ue f
3
### end

### escaped_backslash_in_bracket_word
printf '[%s]' [\\]_ "[\\]_"; echo
set -f; printf '[%s]' [\\]_; echo
### expect
[[\]_][[\]_]
[[\]_]
### end

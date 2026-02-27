# Word splitting tests
# Inspired by Oils spec/word-split.test.sh
# https://github.com/oilshell/oil/blob/master/spec/word-split.test.sh

### ws_ifs_scoped
# IFS is scoped with local
### skip: TODO IFS-based word splitting not implemented
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
### skip: TODO set -- with word splitting not implemented
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
### skip: TODO set -- with word splitting not implemented
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
### skip: TODO set -- with word splitting not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
fun() { set -- $@; echo "$#:$1:$2:$3"; }
fun "a 1" "b 2"
### expect
4:a:1:b
### end

### ws_quoted_dollar_at
# "$@" preserves arguments
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
set -- "1 2" "3  4"
IFS=
echo "$*"
### expect
1 23  4
### end

### ws_elision_space
# Unquoted whitespace-only var is elided
### skip: TODO word splitting does not elide whitespace-only expansions yet
s1=' '
set -- $s1
echo $#
### expect
0
### end

### ws_elision_nonwhitespace_ifs
# Non-whitespace IFS char produces empty field
### skip: TODO IFS-based word splitting not implemented
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
### skip: TODO IFS-based word splitting not implemented
IFS=_
s1='_a_b_'
set -- $s1
echo "$#:$1:$2:$3"
### expect
3::a:b
### end

### ws_mixed_ifs_whitespace_nonwhitespace
# Mixed whitespace and non-whitespace IFS
### skip: TODO IFS-based word splitting not implemented
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
### skip: TODO IFS-based word splitting not implemented
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
### skip: TODO IFS-based word splitting not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
fun() { set -- 1 $@ $* 2; echo $#; }
fun
### expect
2
### end

### ws_unquoted_empty_elided
# unquoted empty var is elided
### skip: TODO word elision not implemented
empty=""
set -- 1 $empty 2
echo $#
### expect
2
### end

### ws_unquoted_whitespace_elided
# unquoted whitespace var is elided
### skip: TODO word elision not implemented
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
### skip: TODO IFS-based word splitting not implemented
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
### skip: TODO IFS-based word splitting not implemented
IFS=''
x="a b	c"
set -- $x
echo "$#:$1"
### expect
1:a b	c
### end

### ws_ifs_unset_default
# IFS unset behaves like space/tab/newline
### skip: TODO IFS-based word splitting not implemented
unset IFS
x="a b	c"
set -- $x
echo "$#:$1:$2:$3"
### expect
3:a:b:c
### end

### ws_ifs_backslash
# IFS=backslash splits on backslash
### skip: TODO IFS-based word splitting not implemented
IFS='\'
s='a\b'
set -- $s
echo "$#:$1:$2"
### expect
2:a:b
### end

### ws_ifs_glob_metachar_star
# IFS characters that are glob metacharacters
### skip: TODO IFS-based word splitting not implemented
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
### skip: TODO IFS-based word splitting not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO $@/$* splitting/joining not implemented
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
### skip: TODO IFS-based word splitting not implemented
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
### skip: TODO set -- with word splitting not implemented
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

# BashBox sed cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_sed_first_match_only
# first match only
echo aaa | sed 's/a/b/'
### expect
baa
### end

### bashbox_sed_global
# global
echo aaa | sed 's/a/b/g'
### expect
bbb
### end

### bashbox_sed_nth_occurrence
# nth occurrence
echo aaaa | sed 's/a/b/3'
### expect
aaba
### end

### bashbox_sed_nth_occurrence_onwards
# nth occurrence onwards
echo aaaa | sed 's/a/b/2g'
### expect
abbb
### end

### bashbox_sed_case_insensitive_blanks_between_flags
# case insensitive, blanks between flags
echo HeLLo | sed 's/l/x/I g'
### expect
Hexxo
### end

### bashbox_sed_multiline_flag
# multiline flag
printf 'a\nb\n' | sed 'N;s/^/>/Mg'
### expect
>a
>b
### end

### bashbox_sed_bre_groups_and_back_references
# BRE groups and back-references
echo 'hello world' | sed 's/\(hello\) \(world\)/\2 \1/'
### expect
world hello
### end

### bashbox_sed_bre_are_literal
# BRE + ? | ( ) { } are literal
echo 'a+b? (c|d) {e}' | sed 's/a+b? (c|d) {e}/ok/'
### expect
ok
### end

### bashbox_sed_bre_escaped_operators
# BRE escaped operators
echo 'aaa-xy' | sed 's/a\+-\(x\|z\)y\?/!/'
### expect
!
### end

### bashbox_sed_bre_interval
# BRE interval
echo aaaa | sed 's/a\{2\}/X/'
### expect
Xaa
### end

### bashbox_sed_bre_leading_star_is_literal
# BRE leading star is literal
echo '*a' | sed 's/*a/x/'
### expect
x
### end

### bashbox_sed_bre_and_are_literal_mid_pattern
# BRE ^ and $ are literal mid-pattern
echo 'a^b$c' | sed 's/a^b$c/x/'
### expect
x
### end

### bashbox_sed_bre_anchors
# BRE anchors
printf 'ab\nba\n' | sed 's/^a/X/;s/a$/Y/'
### expect
Xb
bY
### end

### bashbox_sed_bre_anchor_before_group_end
# BRE anchor before group end
echo ab | sed 's/\(b$\)/[\1]/'
### expect
a[b]
### end

### bashbox_sed_bre_star_after_group_start_is_literal
# BRE star after group start is literal
echo 'a*' | sed 's/a\(*\)/<\1>/'
### expect
<*>
### end

### bashbox_sed_word_boundaries
# word boundaries
echo 'cat concat' | sed 's/\<cat\>/dog/g'
### expect
dog concat
### end

### bashbox_sed_buffer_anchors
# buffer anchors
echo abc | sed 's/\`a/X/;s/c\'"'"'/Z/'
### expect
XbZ
### end

### bashbox_sed_bracket_with_and_backslash
# bracket with ] and backslash
echo 'a]b\c[d' | sed 's/[]\\[]/_/g'
### expect
a_b_c_d
### end

### bashbox_sed_negated_bracket_with_class
# negated bracket with class
echo 'a1-b2' | sed 's/[^[:alpha:]]//g'
### expect
ab
### end

### bashbox_sed_delimiter_inside_a_bracket
# delimiter inside a bracket
echo 'a/b' | sed 's/[/]/-/'
### expect
a-b
### end

### bashbox_sed_ere_with_e
# ERE with -E
echo abc | sed -E 's/(b|c)+/[&]/'
### expect
a[bc]
### end

### bashbox_sed_ere_with_r
# ERE with -r
echo aa | sed -r 's/a{2}/x/'
### expect
x
### end

### bashbox_sed_ere_escaped_operators_are_literal
# ERE escaped operators are literal
echo 'a+' | sed -E 's/a\+/x/'
### expect
x
### end

### bashbox_sed_replacement_escapes
# replacement escapes
echo 'a.b' | sed 's/\./\n\t\&\\/'
### expect
a
	&\b
### end

### bashbox_sed_whole_match_as_0
# whole match as \0
echo ab | sed 's/b/<\0>/'
### expect
a<b>
### end

### bashbox_sed_unmatched_group_is_empty
# unmatched group is empty
echo ab | sed -E 's/a|(z)/[\1]/'
### expect
[]b
### end

### bashbox_sed_dollar_in_replacement_is_literal
# dollar in replacement is literal
echo a | sed 's/a/$1/'
### expect
$1
### end

### bashbox_sed_case_conversion_u
# case conversion \U
echo 'hello world' | sed 's/\(.*\)/\U\1/'
### expect
HELLO WORLD
### end

### bashbox_sed_case_conversion_u_per_word
# case conversion \u per word
echo 'hello world' | sed 's/\w\+/\u&/g'
### expect
Hello World
### end

### bashbox_sed_case_conversion_l_u
# case conversion \L\u
echo 'HELLO WORLD' | sed 's/.*/\L\u&/'
### expect
Hello world
### end

### bashbox_sed_case_conversion_ends_at_e
# case conversion ends at \E
echo ab | sed 's/\(a\)\(b\)/\U\1\E\2/'
### expect
Ab
### end

### bashbox_sed_case_conversion_l
# case conversion \l
echo ABC | sed 's/.*/\l&/'
### expect
aBC
### end

### bashbox_sed_custom_delimiter_and_escaped_delimiter
# custom delimiter and escaped delimiter
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo 'a|b' | sed 's|a\|b|X|'
### expect
X
### end

### bashbox_sed_tab_escape_in_regex
# tab escape in regex
printf 'a\tb\n' | sed 's/\t/T/'
### expect
aTb
### end

### bashbox_sed_escaped_slash_with_another_delimiter
# escaped slash with another delimiter
echo 'a/b' | sed 's|a\/b|X|'
### expect
X
### end

### bashbox_sed_stray_backslash_before_a_letter_is_literal
# stray backslash before a letter is literal
echo 'ad' | sed 's/a\d/X/'
### expect
X
### end

### bashbox_sed_escaped_slash
# escaped slash
echo 'a/b' | sed 's/\//\/\//'
### expect
a//b
### end

### bashbox_sed_newline_escape_in_regex
# newline escape in regex
printf 'a\nb\n' | sed 'N;s/a\nb/x/'
### expect
x
### end

### bashbox_sed_escaped_newline_in_replacement
# escaped newline in replacement
echo a | sed 's/a/x\
y/'
### expect
x
y
### end

### bashbox_sed_empty_regex_matches
# empty regex matches
echo abc | sed 's/x*/-/g'
### expect
-a-b-c-
### end

### bashbox_sed_empty_regex_reuses_the_last_one
# empty regex reuses the last one
echo xaa | sed '/x/s//y/;s//z/'
### expect
yaa
### end

### bashbox_sed_p_flag
# p flag
printf 'a\nb\n' | sed -n 's/b/B/p'
### expect
B
### end

### bashbox_sed_commands_separated_by_and_newlines
# commands separated by ; and newlines
echo abc | sed 's/a/1/; s/b/2/ g
s/c/3/'
### expect
123
### end

### bashbox_sed_comments
# comments
echo a | sed '# note
p#c'
### expect
a
a
### end

### bashbox_sed_several_e
# several -e
echo abc | sed -e 's/a/1/' -e's/c/3/'
### expect
1b3
### end

### bashbox_sed_clustered_ne
# clustered -ne
printf 'a\nb\n' | sed -ne 's/b/B/p'
### expect
B
### end

### bashbox_sed_n_without_p_prints_nothing
# -n without p prints nothing
echo a | sed -n 's/a/b/'
### expect
### end

### bashbox_sed_u_is_accepted
# -u is accepted
echo a | sed -u s/a/b/
### expect
b
### end

### bashbox_sed_empty_script
# empty script
echo a | sed ''
### expect
a
### end

### bashbox_sed_empty_input
# empty input
printf '' | sed p
### expect
### end

### bashbox_sed_line
# line
printf 'a\nb\nc\n' | sed 2d
### expect
a
c
### end

### bashbox_sed_last_line
# last line
printf 'a\nb\nc\n' | sed '$d'
### expect
a
b
### end

### bashbox_sed_negation_with_blanks
# negation with blanks
printf 'a\nb\nc\n' | sed '2 ! d'
### expect
b
### end

### bashbox_sed_regex
# regex
printf 'a\nb\n' | sed '/b/d'
### expect
a
### end

### bashbox_sed_custom_regex_delimiter
# custom regex delimiter
printf 'a\nb\n' | sed '\%a%d'
### expect
b
### end

### bashbox_sed_regex_with_i
# regex with I
printf 'a\nB\n' | sed '/b/Id'
### expect
a
### end

### bashbox_sed_regex_with_m
# regex with M
printf 'a\nb\n' | sed -n 'N;/^b/Mp'
### expect
a
b
### end

### bashbox_sed_delimiter_inside_bracket
# delimiter inside bracket
printf 'a/b\nc\n' | sed '/[/]/d'
### expect
c
### end

### bashbox_sed_first_step
# first~step
printf '1\n2\n3\n4\n5\n' | sed '1~3d'
### expect
2
3
5
### end

### bashbox_sed_zero_first_step
# zero first~step
printf '1\n2\n3\n4\n' | sed '0~2d'
### expect
1
3
### end

### bashbox_sed_first_0
# first~0
printf '1\n2\n3\n' | sed -n '2~0p'
### expect
2
### end

### bashbox_sed_line_range
# line range
printf 'a\nb\nc\n' | sed '2,$d'
### expect
a
### end

### bashbox_sed_regex_range
# regex range
printf 'a\nb\nc\nd\n' | sed '/b/,/c/d'
### expect
a
d
### end

### bashbox_sed_range_end_is_checked_from_the_next_line
# range end is checked from the next line
printf '1\n2\n3\n4\n' | sed '2,/./d'
### expect
1
4
### end

### bashbox_sed_range_end_before_start_is_one_line
# range end before start is one line
printf 'a\nb\nc\n' | sed -n '2,1p'
### expect
b
### end

### bashbox_sed_range_restarts
# range restarts
printf 'a\nb\na\nc\n' | sed -n '/a/,/b/p'
### expect
a
b
a
c
### end

### bashbox_sed_negated_range
# negated range
printf '1\n2\n3\n' | sed '1,2!d'
### expect
1
2
### end

### bashbox_sed_0_re_can_end_on_line_1
# 0,/re/ can end on line 1
printf '1\n2\n1\n' | sed '0,/1/d'
### expect
2
1
### end

### bashbox_sed_addr_n
# addr,+N
printf '1\n2\n3\n4\n' | sed '/2/,+1d'
### expect
1
4
### end

### bashbox_sed_addr_0
# addr,+0
printf '1\n2\n3\n' | sed '/2/,+0d'
### expect
1
3
### end

### bashbox_sed_addr_n_2
# addr,~N
printf '1\n2\n3\n4\n5\n' | sed '2,~4d'
### expect
1
5
### end

### bashbox_sed_addr_n_on_a_multiple
# addr,~N on a multiple
printf '1\n2\n3\n4\n5\n6\n7\n' | sed '4,~2d'
### expect
1
2
3
7
### end

### bashbox_sed_addr_0_2
# addr,~0
printf '1\n2\n3\n' | sed '2,~0d'
### expect
1
3
### end

### bashbox_sed_p
# p
echo a | sed 'p;p'
### expect
a
a
a
### end

### bashbox_sed_n
# -n \$=
printf 'a\nb\nc\n' | sed -n '$='
### expect
3
### end

### bashbox_sed_a_one_liner
# a one-liner
printf 'a\nb\n' | sed '1a X'
### expect
a
X
b
### end

### bashbox_sed_a_with_backslash_newline
# a with backslash-newline
printf 'a\nb\n' | sed 'a\
X'
### expect
a
X
b
X
### end

### bashbox_sed_a_with_several_lines
# a with several lines
printf 'a\nb\n' | sed '1a\
foo\
bar'
### expect
a
foo
bar
b
### end

### bashbox_sed_a_keeps_blanks_after_backslash
# a keeps blanks after backslash
echo a | sed 'a\  X'
### expect
a
  X
### end

### bashbox_sed_a_drops_a_trailing_backslash
# a drops a trailing backslash
echo a | sed 'a X\'
### expect
a
X
### end

### bashbox_sed_a_with_nothing_after_the_backslash
# a with nothing after the backslash
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo a | sed 'a\'
### expect
a
### end

### bashbox_sed_a_after_a_last_line_without_newline
# a after a last line without newline
printf 'a' | sed '$a END'
### expect
a
END
### end

### bashbox_sed_a_text_runs_to_the_end_of_line
# a text runs to the end of line
printf '1\n2\n' | sed '1a X;q'
### expect
1
X;q
2
### end

### bashbox_sed_c
# c
printf 'a\nb\n' | sed '$c\
Z'
### expect
a
Z
### end

### bashbox_sed_c_on_a_range_prints_once
# c on a range prints once
printf '1\n2\n3\n4\n' | sed '2,3c Z'
### expect
1
Z
4
### end

### bashbox_sed_c_with_negation
# c with negation
printf '1\n2\n3\n' | sed '2!c X'
### expect
X
2
X
### end

### bashbox_sed_y
# y
echo aabbcc | sed 'y/abc/xyz/'
### expect
xxyyzz
### end

### bashbox_sed_y_escapes
# y escapes
echo 'a/b' | sed 'y/a\/b/x\\y/'
### expect
x\y
### end

### bashbox_sed_y_with_newline
# y with newline
printf 'a\nb\n' | sed 'N;y/\n/ /'
### expect
a b
### end

### bashbox_sed_q_prints_and_stops
# q prints and stops
printf 'a\nb\n' | sed 1q
### expect
a
### end

### bashbox_sed_q_does_not_print
# Q does not print
printf 'a\nb\nc\n' | sed 2Q
### expect
a
### end

### bashbox_sed_q_drops_queued_appends
# Q drops queued appends
echo a | sed 'a X
Q'
### expect
### end

### bashbox_sed_n_2
# n
printf '1\n2\n3\n4\n5\n' | sed 'n;d'
### expect
1
3
5
### end

### bashbox_sed_n_quiet
# n quiet
printf 'a\nb\nc\n' | sed -n 'n;p'
### expect
b
### end

### bashbox_sed_n_at_the_last_line
# n at the last line
echo a | sed 'n;s/a/x/'
### expect
a
### end

### bashbox_sed_n_joins_lines
# N joins lines
printf '1\n2\n3\n' | sed '$!N;s/\n/-/'
### expect
1-2
3
### end

### bashbox_sed_n_at_the_last_line_prints
# N at the last line prints
printf '1\n2\n3\n' | sed 'N;s/\n/,/'
### expect
1,2
3
### end

### bashbox_sed_p_and_d
# P and D
printf '1\n2\n3\n' | sed '$!N;P;D'
### expect
1
2
3
### end

### bashbox_sed_d_without_newline_acts_like_d
# D without newline acts like d
printf '1\n2\n' | sed D
### expect
### end

### bashbox_sed_d_restarts_the_cycle
# D restarts the cycle
printf '1\n2\n3\n' | sed 'N;D'
### expect
3
### end

### bashbox_sed_h_and_g
# h and G
printf 'a\nb\n' | sed 'h;G'
### expect
a
a
b
b
### end

### bashbox_sed_h_x_and_g
# H, x and g
printf 'a\nb\nc\n' | sed '1h;2,$H;$!d;x;s/\n/,/g'
### expect
a,b,c
### end

### bashbox_sed_reverse_lines
# reverse lines
printf '1\n2\n3\n' | sed '1!G;h;$!d'
### expect
3
2
1
### end

### bashbox_sed_x
# x
printf 'a\nb\n' | sed x
### expect

a
### end

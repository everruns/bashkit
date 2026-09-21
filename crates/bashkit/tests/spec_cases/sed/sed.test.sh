### sed_substitute
# Basic substitution
printf 'hello world\n' | sed 's/world/there/'
### expect
hello there
### end

### sed_substitute_global
# Global substitution
printf 'aaa\n' | sed 's/a/b/g'
### expect
bbb
### end

### sed_substitute_first
# First occurrence only
printf 'aaa\n' | sed 's/a/b/'
### expect
baa
### end

### sed_delete
# Delete line
printf 'one\ntwo\nthree\n' | sed '2d'
### expect
one
three
### end

### sed_delete_pattern
# Delete by pattern
printf 'foo\nbar\nbaz\n' | sed '/bar/d'
### expect
foo
baz
### end

### sed_print
# Print specific line
printf 'one\ntwo\nthree\n' | sed -n '2p'
### expect
two
### end

### sed_last_line
# Address last line
printf 'one\ntwo\nthree\n' | sed '$d'
### expect
one
two
### end

### sed_range
# Line range
printf 'a\nb\nc\nd\n' | sed '2,3d'
### expect
a
d
### end

### sed_ampersand
# Ampersand replacement
printf 'hello\n' | sed 's/hello/[&]/'
### expect
[hello]
### end

### sed_regex_group
# Regex groups
printf 'hello world\n' | sed 's/\(hello\) \(world\)/\2 \1/'
### expect
world hello
### end

### sed_case_insensitive
# Case insensitive substitution
printf 'Hello World\n' | sed 's/hello/hi/i'
### expect
hi World
### end

### sed_delimiter
# Alternative delimiter
printf 'path/to/file\n' | sed 's|/|_|g'
### expect
path_to_file
### end

### sed_multiple
# Multiple commands separated by semicolons
printf 'hello world\n' | sed 's/hello/hi/; s/world/there/'
### expect
hi there
### end

### sed_quit
# Quit command
printf 'one\ntwo\nthree\n' | sed '2q'
### expect
one
two
### end

### sed_regex_class
# Character class
printf 'a1b2c3\n' | sed 's/[0-9]//g'
### expect
abc
### end

### sed_append
# Append text after matching line
printf 'one\ntwo\n' | sed '/one/a\inserted'
### expect
one
inserted
two
### end

### sed_insert
# Insert text before matching line
printf 'one\ntwo\n' | sed '/two/i\inserted'
### expect
one
inserted
two
### end

### sed_nth_occurrence
# Replace 2nd occurrence
printf 'aaa\n' | sed 's/a/X/2'
### expect
aXa
### end

### sed_nth_occurrence_3rd
# Replace 3rd occurrence
printf 'aaaa\n' | sed 's/a/X/3'
### expect
aaXa
### end

### sed_print_range
# Print range of lines
printf 'a\nb\nc\nd\n' | sed -n '2,3p'
### expect
b
c
### end

### sed_line_number
# Substitute on specific line
printf 'a\nb\na\n' | sed '2s/b/X/'
### expect
a
X
a
### end

### sed_line_range_subst
# Substitute on line range
printf 'a\nb\nc\nd\n' | sed '2,3s/./X/'
### expect
a
X
X
d
### end

### sed_multiple_e_flags
# Multiple -e expressions
printf 'hello world\n' | sed -e 's/hello/hi/' -e 's/world/there/'
### expect
hi there
### end

### sed_inplace
# In-place editing
echo 'test' > /tmp/sedtest.txt && sed -i 's/test/done/' /tmp/sedtest.txt && cat /tmp/sedtest.txt
### expect
done
### end

### sed_extended_regex_plus
# Extended regex with + quantifier
printf 'aaa\n' | sed -E 's/a+/X/'
### expect
X
### end

### sed_extended_regex_question
# Extended regex with ? quantifier
printf 'ab\n' | sed -E 's/ab?/X/'
### expect
X
### end

### sed_extended_regex_group
# Extended regex with capture groups
printf 'hello world\n' | sed -E 's/(hello) (world)/\2 \1/'
### expect
world hello
### end

### sed_extended_regex_alternation
# Extended regex with alternation
printf 'cat\ndog\nbird\n' | sed -E '/cat|dog/d'
### expect
bird
### end

### sed_hold_h
# Hold space with grouped commands
printf 'a\nb\n' | sed '1h; 2{x;p;x}'
### expect
a
a
b
### end

### sed_hold_H
# Hold space H append with multi-command pipeline
printf 'a\nb\nc\n' | sed 'H; $!d; x; s/\n/ /g'
### expect
 a b c
### end

### sed_exchange_x
printf 'a\nb\n' | sed 'x'
### expect

a
### end

### sed_change
printf 'one\ntwo\nthree\n' | sed '2c\replaced'
### expect
one
replaced
three
### end

### sed_quit_Q
# Q (quiet quit) exits without printing current line
printf 'a\nb\nc\n' | sed '2Q'
### expect
a
### end

### sed_branch_t
# Branch on substitution with label
printf 'abc\n' | sed ':loop; s/a/X/; t loop'
### expect
Xbc
### end

### sed_grouped_commands
# Grouped commands with address
printf 'a\nb\nc\n' | sed '2{s/b/X/;p}'
### expect
a
X
X
c
### end

### sed_dollar_last_line_subst
# Substitute on last line
printf 'a\nb\nc\n' | sed '$s/c/X/'
### expect
a
b
X
### end

### sed_negate_pattern
# Address negation with !
printf 'foo\nbar\nbaz\n' | sed '/bar/!d'
### expect
bar
### end

### sed_regex_any_char
# Any character match
printf 'abc\n' | sed 's/./-/g'
### expect
---
### end

### sed_regex_start_anchor
# Start of line anchor
printf 'aaa\n' | sed 's/^a/X/'
### expect
Xaa
### end

### sed_regex_end_anchor
# End of line anchor
printf 'aaa\n' | sed 's/a$/X/'
### expect
aaX
### end

### sed_regex_star
# Zero or more matches
printf 'aaa\n' | sed 's/a*/X/'
### expect
X
### end

### sed_regex_escaped_plus
# Escaped plus in BRE mode
printf 'aaa\n' | sed 's/a\+/X/'
### expect
X
### end

### sed_backref_1
# Single backreference
printf 'hello\n' | sed 's/\(hel\)lo/\1p/'
### expect
help
### end

### sed_backref_2
# Multiple backreferences
printf 'abcd\n' | sed 's/\(ab\)\(cd\)/\2\1/'
### expect
cdab
### end

### sed_search_backref_1
# Backreference in search pattern (match repeated group)
printf '<a href="tag_hello">hello</a>\n' | sed 's|<a href="tag_\([^"]*\)">\1</a>|\1|g'
### expect
hello
### end

### sed_search_backref_2
# Simple search-side backreference (repeated char)
printf 'aabbc\n' | sed 's/\(.\)\1/X/g'
### expect
XXc
### end

### sed_empty_replacement
# Empty replacement (delete match)
printf 'hello\n' | sed 's/l//g'
### expect
heo
### end

### sed_literal_newline
printf 'a b\n' | sed 's/ /\n/'
### expect
a
b
### end

### sed_escaped_slash
# Escaped delimiter in pattern
printf 'a/b\n' | sed 's/\//X/'
### expect
aXb
### end

### sed_character_class_alpha
# Alpha character class
printf 'a1b2\n' | sed 's/[[:alpha:]]//g'
### expect
12
### end

### sed_character_class_digit
# Digit character class
printf 'a1b2\n' | sed 's/[[:digit:]]//g'
### expect
ab
### end

### sed_negated_class
# Negated character class
printf 'a1b2c3\n' | sed 's/[^0-9]//g'
### expect
123
### end

### sed_range_class
# Range in character class
printf 'AbCdE\n' | sed 's/[A-Z]/_/g'
### expect
_b_d_
### end

### sed_address_pattern_subst
# Substitute only on matching lines
printf 'foo bar\nbaz qux\nfoo baz\n' | sed '/foo/s/bar/XXX/'
### expect
foo XXX
baz qux
foo baz
### end

### sed_address_not_pattern_subst
# Address negation with substitution
printf 'foo\nbar\nbaz\n' | sed '/foo/!s/./X/g'
### expect
foo
XXX
XXX
### end

### sed_multiple_patterns
printf 'a\nb\nc\nd\n' | sed '/a/,/c/d'
### expect
d
### end

### sed_print_silent_range
# Silent mode with range print
printf 'a\nb\nc\nd\n' | sed -n '2,3p'
### expect
b
c
### end

### sed_print_duplicate
# Print causes duplicate output
printf 'a\nb\n' | sed '1p'
### expect
a
a
b
### end

### sed_delete_first
# Delete first line
printf 'a\nb\nc\n' | sed '1d'
### expect
b
c
### end

### sed_delete_range_pattern
printf 'a\nb\nc\nd\n' | sed '/b/,$d'
### expect
a
### end

### sed_substitute_global_line
# Combine global and line address
printf 'aaa\nbbb\naaa\n' | sed '1s/a/X/g'
### expect
XXX
bbb
aaa
### end

### sed_empty_input
# Handle empty input
printf '' | sed 's/x/y/'
### expect
### end

### sed_special_chars_in_replacement
printf 'hello\n' | sed 's/hello/a&b/'
### expect
ahellob
### end

### sed_escaped_ampersand
# Escaped ampersand in replacement
printf 'hello\n' | sed 's/hello/\&/'
### expect
&
### end

### sed_step_address
# Step address: delete every 2nd line
printf 'a\nb\nc\nd\ne\nf\n' | sed '0~2d'
### expect
a
c
e
### end

### sed_zero_address
# 0,/pattern/ addressing: substitute only first match
printf 'no\nyes\nyes\n' | sed '0,/yes/s/yes/FIRST/'
### expect
no
FIRST
yes
### end

### sed_pattern_range
printf 'a\nstart\nb\nend\nc\n' | sed '/start/,/end/d'
### expect
a
c
### end

### sed_group_delete
# Grouped commands: address with delete
printf 'a\nb\nc\n' | sed '2{d}'
### expect
a
c
### end

### sed_group_nested_hold
# Grouped commands with hold space operations
printf 'x\ny\n' | sed '1{h;d}; 2{x;p;x}'
### expect
x
y
### end

### sed_branch_b_unconditional
# Unconditional branch to end
printf 'a\nb\nc\n' | sed '2b; s/./X/'
### expect
X
b
X
### end

### sed_branch_t_no_match
# t does NOT branch when substitution fails
printf 'abc\n' | sed ':top; s/z/Z/; t top; s/a/X/'
### expect
Xbc
### end

### sed_Q_first_line
# Q on first line prints nothing
printf 'a\nb\n' | sed '1Q'
### expect
### end

### sed_step_address_1_2
# Step address: every 2nd line starting at line 1
printf 'a\nb\nc\nd\n' | sed '1~2s/.*/X/'
### expect
X
b
X
d
### end

### sed_zero_address_first_line_match
# 0,/pattern/ where first line matches
printf 'yes\nyes\nno\n' | sed '0,/yes/s/yes/FIRST/'
### expect
FIRST
yes
no
### end

### sed_group_with_regex_addr
# Grouped commands with regex address
printf 'foo\nbar\nbaz\n' | sed '/bar/{s/bar/BAR/;p}'
### expect
foo
BAR
BAR
baz
### end

### sed_unescape_slash_in_replacement
# \/ in replacement should produce literal /
echo "abc" | sed 's/b/\//'
### expect
a/c
### end

### sed_regex_groups_with_slash
# back-references with \/ in replacement
echo "2026-01-15" | sed 's/\([0-9]*\)-\([0-9]*\)-\([0-9]*\)/\3\/\2\/\1/'
### expect
15/01/2026
### end

### sed_escaped_backslash_in_replacement
# \\ in replacement should produce literal backslash
echo "abc" | sed 's/b/\\/'
### expect
a\c
### end

### sed_replacement_dollar_is_literal
# Issue #2427 A: `$` in the RHS is text, not a capture reference
printf 'ab\n' | sed 's/a/$x/'
printf 'ab\n' | sed 's/\(a\)/[$1]/'
printf 'ab\n' | sed 's/a/$$/'
### expect
$xb
[$1]b
$$b
### end

### sed_replacement_backslash_drops
# Issue #2427 A: GNU drops the backslash before an ordinary character
printf 'ab\n' | sed 's/a/\$/'
printf 'ab\n' | sed 's/a/\q/'
### expect
$b
qb
### end

### sed_replacement_case_conversion
# GNU \U \L \u \l \E in the replacement
printf 'abc\n' | sed 's/a\(b\)c/\U\1x\E-\1/'
printf 'abc\n' | sed 's/.*/\u&/'
printf 'ABC\n' | sed 's/.*/\L&/'
### expect
BX-b
Abc
abc
### end

### sed_bre_literals
# Issue #2427 B: + ? | are ordinary characters in BRE
printf 'a+b\n' | sed 's/a+b/X/'
printf 'a?b\n' | sed 's/a?b/X/'
printf 'a|b\n' | sed 's/a|b/X/'
printf 'aaa\n' | sed 's/*a/X/'
### expect
X
X
X
aaa
### end

### sed_bre_anchors_are_positional
# Issue #2427 B: ^ and $ only anchor at the edges of a BRE
printf 'a^b\n' | sed -n '/a^b/p'
printf 'a$b\n' | sed -n '/a$b/p'
### expect
a^b
a$b
### end

### sed_address_regex_uses_bre
# Issue #2427 B: address regexes take the same BRE path as s///
printf 'aaa\n' | sed -n '/a\+/p'
printf 'a+b\n' | sed -n '/a+b/p'
### expect
aaa
a+b
### end

### sed_bracket_expression_holds_delimiter
# Inside [...] the s/// delimiter is an ordinary character
printf 'a/b\n' | sed 's/[/]/X/'
printf 'a]b\n' | sed 's/[]]/X/'
### expect
aXb
aXb
### end

### sed_regex_to_line_range
# Issue #2427 C: /re/,N must keep the regex
printf 'a\nb\nc\nd\n' | sed '/b/,3d'
### expect
a
d
### end

### sed_closed_range_stays_closed
# Issue #2427 C: N,/re/ closes once and does not re-open
printf 'a\nb\nc\n' | sed -n '1,/b/p'
### expect
a
b
### end

### sed_zero_range_tests_first_line
# 0,/re/ is the form whose end regex is tested on line 1
printf 'b\nb\nc\n' | sed -n '0,/b/p'
### expect
b
### end

### sed_relative_end_addresses
# addr,+N and addr,~N
printf '1\n2\n3\n4\n5\n6\n7\n8\n9\n' | sed -n '3,+2p'
printf '1\n2\n3\n4\n5\n6\n7\n8\n9\n' | sed -n '2,~4p'
### expect
3
4
5
2
3
4
### end

### sed_change_on_range_prints_once
# Issue #2427 C: c on a range emits its text once
printf 'a\nb\nc\n' | sed '1,2c\Z'
### expect
Z
c
### end

### sed_multiple_files_are_one_stream
# Issue #2427 D: line numbers, $ and q span all operands
printf 'a\nb\nc\n' > /tmp/sed_f1.txt
printf 'd\ne\n' > /tmp/sed_f2.txt
sed -n '$p' /tmp/sed_f1.txt /tmp/sed_f2.txt
sed '2q' /tmp/sed_f1.txt /tmp/sed_f2.txt
sed -n '$=' /tmp/sed_f1.txt /tmp/sed_f2.txt
### expect
e
a
b
5
### end

### sed_separate_restores_per_file_streams
# -s puts each operand back in its own stream
printf 'a\nb\nc\n' > /tmp/sed_s1.txt
printf 'd\ne\n' > /tmp/sed_s2.txt
sed -s -n '$p' /tmp/sed_s1.txt /tmp/sed_s2.txt
### expect
c
e
### end

### sed_missing_final_newline_is_preserved
# Issue #2427 E: sed must not invent a trailing newline
printf 'a' | sed 's/a/b/' > /tmp/sed_nl.txt
wc -c < /tmp/sed_nl.txt
printf 'abc' > /tmp/sed_inplace.txt
sed -i 's/b/X/' /tmp/sed_inplace.txt
wc -c < /tmp/sed_inplace.txt
cat /tmp/sed_inplace.txt
echo
### expect
1
3
aXc
### end

### sed_in_place_preserves_mode
# Issue #2427 E: -i keeps the original file mode
printf 'x\n' > /tmp/sed_mode.txt
chmod 600 /tmp/sed_mode.txt
sed -i 's/x/y/' /tmp/sed_mode.txt
stat -c %a /tmp/sed_mode.txt
cat /tmp/sed_mode.txt
### expect
600
y
### end

### sed_in_place_backup_suffix
# -i.bak keeps the original alongside the edit
printf 'a\nb\n' > /tmp/sed_bak.txt
sed -i.bak 's/a/A/' /tmp/sed_bak.txt
cat /tmp/sed_bak.txt
cat /tmp/sed_bak.txt.bak
### expect
A
b
a
b
### end

### sed_occurrence_with_global
# Issue #2427 F: s///Ng replaces the Nth match and everything after it
printf 'heLLo\n' | sed 's/L/x/2g'
printf 'aaa\n' | sed 's/a/X/3g'
printf 'aaaa\n' | sed 's/a/X/3'
### expect
heLxo
aaX
aaXa
### end

### sed_line_number_command
# Issue #2427 G: `=`
printf 'a\nb\n' | sed -n '$='
### expect
2
### end

### sed_next_commands
# Issue #2427 G: n and N
printf 'a\nb\nc\n' | sed 'N;s/\n/ /'
printf 'a\nb\nc\n' | sed -n '/a/{n;p}'
### expect
a b
c
b
### end

### sed_transliterate
# Issue #2427 G: y
printf 'abc\n' | sed 'y/abc/xyz/'
### expect
xyz
### end

### sed_comments_and_hash_n
# Issue #2427 G: # comments, and #n as the first line implying -n
printf 'a\nb\n' | sed '# just a comment'
printf 'a\n' | sed '#n
p'
### expect
a
b
a
### end

### sed_list_command
# Issue #2427 G: l renders the pattern space unambiguously
printf 'a\tb\\c\n' | sed -n 'l'
### expect
a\tb\\c$
### end

### sed_branch_if_no_substitution
# Issue #2427 G: T
printf 'abc\n' | sed 's/a/X/;T end;s/b/Y/;:end'
printf 'abc\n' | sed 'T end;s/a/X/;:end'
### expect
XYc
abc
### end

### sed_read_and_write_files
# Issue #2427 G: r, R and w against the VFS
printf 'R1\nR2\n' > /tmp/sed_r.txt
printf 'a\nb\n' | sed '1r /tmp/sed_r.txt'
printf 'a\nb\n' | sed 'R /tmp/sed_r.txt'
printf 'a\nb\n' | sed -n '1w /tmp/sed_w.txt'
cat /tmp/sed_w.txt
### expect
a
R1
R2
b
a
R1
b
R2
a
### end

### sed_quit_exit_status
# q and Q carry an exit status
printf 'a\nb\n' | sed '1q5'
echo "rc=$?"
printf 'a\nb\n' | sed '1Q3'
echo "rc=$?"
### expect
a
rc=5
rc=3
### end

### sed_multibyte_delimiter_does_not_crash
# Issue #2427 H (TM-UNI-002): a multi-byte s delimiter is parsed, not sliced
### bash_diff: L-SED-001 - GNU rejects a multi-byte delimiter; Bashkit accepts it
printf 'a\n' | sed 's≠a≠X≠'
### expect
X
### end

### sed_clustered_short_options
# Issue #2427 I: -ne must behave like -n -e
printf 'a\n' | sed -ne p
### expect
a
### end

### sed_empty_script_is_passthrough
# Issue #2427 I: an empty script copies input through
printf 'a\n' | sed ''
echo "rc=$?"
### expect
a
rc=0
### end

### sed_script_file
# Issue #2427 I: -f reads the script from a file
printf 's/a/X/\ns/X/Y/\n' > /tmp/sed_script.sed
printf 'a\n' | sed -f /tmp/sed_script.sed
### expect
Y
### end

### sed_long_options
# Issue #2427 I: long spellings
printf 'x\n' | sed --expression='s/x/y/'
printf 'x\n' | sed --quiet -e p
### expect
y
x
### end

### sed_change_range_needs_a_real_end
# A range that runs off the end of input never "ends", so c emits nothing
printf 'a\nb\n' | sed '1,5c\Z'
printf 'a\nb\nc\n' | sed '2,5c\Z'
printf 'a\nb\nc\n' | sed '/a/,/zz/c\Z'
### expect
a
### end

### sed_quit_takes_one_address
# q and Q stop the stream, so a range is a compile error
printf 'a\nb\n' | sed '1,2q' 2>&1
echo "rc=$?"
### expect
sed: -e expression #1, char 4: command only uses one address
rc=1
### end

### sed_invalid_backreference_is_rejected
# GNU rejects \1 with no group instead of substituting an empty string
printf 'a\n' | sed 's/a/\1/' 2>&1
echo "rc=$?"
### expect
sed: -e expression #1, char 7: invalid reference \1 on `s' command's RHS
rc=1
### end

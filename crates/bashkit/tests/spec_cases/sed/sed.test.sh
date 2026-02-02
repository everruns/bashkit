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
### skip: -i flag not implemented
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
### skip: hold space commands not implemented
printf 'a\nb\n' | sed '1h; 2{x;p;x}'
### expect
a
a
b
### end

### sed_hold_H
### skip: hold space commands not implemented
printf 'a\nb\nc\n' | sed 'H; $!d; x; s/\n/ /g'
### expect
 a b c
### end

### sed_exchange_x
### skip: hold space commands not implemented
printf 'a\nb\n' | sed 'x'
### expect

a
### end

### sed_change
### skip: change command not implemented
printf 'one\ntwo\nthree\n' | sed '2c\replaced'
### expect
one
replaced
three
### end

### sed_quit_Q
### skip: Q command not implemented
printf 'a\nb\nc\n' | sed '2Q'
### expect
a
### end

### sed_branch_t
### skip: branching not implemented
printf 'abc\n' | sed ':loop; s/a/X/; t loop'
### expect
Xbc
### end

### sed_grouped_commands
### skip: grouped commands not implemented
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
### skip: backreferences not working correctly
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

### sed_empty_replacement
# Empty replacement (delete match)
printf 'hello\n' | sed 's/l//g'
### expect
heo
### end

### sed_literal_newline
### skip: literal newline in replacement not implemented
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
### skip: pattern range addresses not implemented
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
### skip: pattern range addresses not implemented
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
### skip: ampersand with adjacent chars not working
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
### skip: step address not implemented
printf 'a\nb\nc\nd\ne\nf\n' | sed '0~2d'
### expect
a
c
e
### end

### sed_first_match
### skip: 0,/pattern/ address not implemented
printf 'no\nyes\nyes\n' | sed '0,/yes/s/yes/FIRST/'
### expect
no
FIRST
yes
### end

### sed_pattern_range_inclusive
### skip: pattern range addresses not implemented
printf 'a\nstart\nb\nend\nc\n' | sed '/start/,/end/d'
### expect
a
c
### end

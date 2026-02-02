### grep_basic
# Basic pattern match
printf 'foo\nbar\nbaz\n' | grep bar
### expect
bar
### end

### grep_multiple
# Multiple matches
printf 'foo\nbar\nfoo\n' | grep foo
### expect
foo
foo
### end

### grep_no_match
# No match returns exit code 1
printf 'foo\nbar\n' | grep xyz
### exit_code: 1
### expect
### end

### grep_case_insensitive
# Case insensitive search
printf 'Hello\nWORLD\n' | grep -i hello
### expect
Hello
### end

### grep_invert
# Invert match
printf 'foo\nbar\nbaz\n' | grep -v bar
### expect
foo
baz
### end

### grep_line_numbers
# Show line numbers
printf 'foo\nbar\nbaz\n' | grep -n bar
### expect
2:bar
### end

### grep_count
# Count matches
printf 'foo\nbar\nfoo\n' | grep -c foo
### expect
2
### end

### grep_fixed_string
# Fixed string (no regex)
printf 'a.b\na*b\n' | grep -F 'a.b'
### expect
a.b
### end

### grep_regex
# Regex pattern
printf 'cat\ncar\nbar\n' | grep 'ca.'
### expect
cat
car
### end

### grep_anchor_start
# Start anchor
printf 'foo\nbar\nfoobar\n' | grep '^foo'
### expect
foo
foobar
### end

### grep_anchor_end
# End anchor
printf 'foo\nbar\nfoobar\n' | grep 'bar$'
### expect
bar
foobar
### end

### grep_extended
# Extended regex
printf 'color\ncolour\n' | grep -E 'colou?r'
### expect
color
colour
### end

### grep_word
# Word boundary match
printf 'foo\nfoobar\nbar foo baz\n' | grep -w foo
### expect
foo
bar foo baz
### end

### grep_only_matching
# Only show matching part
printf 'hello world\n' | grep -o 'world'
### expect
world
### end

### grep_files_with_matches
# List matching files (shows (stdin) for stdin input)
printf 'foo\nbar\n' | grep -l foo
### expect
(stdin)
### end

### grep_quiet
### skip: -q flag still outputs matches
printf 'foo\nbar\n' | grep -q foo
### exit_code: 0
### expect
### end

### grep_quiet_no_match
# Quiet mode with no match
printf 'foo\nbar\n' | grep -q xyz
### exit_code: 1
### expect
### end

### grep_max_count
### skip: -m flag not implemented
printf 'foo\nfoo\nfoo\n' | grep -m 2 foo
### expect
foo
foo
### end

### grep_after_context
### skip: -A context flag not implemented
printf 'a\nfoo\nb\nc\n' | grep -A 1 foo
### expect
foo
b
### end

### grep_before_context
### skip: -B context flag not implemented
printf 'a\nb\nfoo\nc\n' | grep -B 1 foo
### expect
b
foo
### end

### grep_context
### skip: -C context flag not implemented
printf 'a\nb\nfoo\nc\nd\n' | grep -C 1 foo
### expect
b
foo
c
### end

### grep_recursive
### skip: recursive grep not implemented for virtual fs
grep -r pattern /some/dir
### exit_code: 1
### expect
### end

### grep_multiple_patterns
### skip: multiple -e patterns not implemented
printf 'foo\nbar\nbaz\n' | grep -e foo -e bar
### expect
foo
bar
### end

### grep_pattern_file
### skip: -f pattern file not implemented
printf 'foo\nbar\nbaz\n' | grep -f /patterns.txt
### expect
foo
bar
### end

### grep_null_data
### skip: -z null-terminated not implemented
printf 'foo\0bar\0' | grep -z foo
### expect
foo
### end

### grep_only_matching_multiple
# Multiple matches per line
printf 'foo bar foo\n' | grep -o foo
### expect
foo
foo
### end

### grep_regex_star
# Zero or more
printf 'ac\nabc\nabbc\n' | grep 'ab*c'
### expect
ac
abc
abbc
### end

### grep_regex_plus_extended
# One or more with -E
printf 'ac\nabc\nabbc\n' | grep -E 'ab+c'
### expect
abc
abbc
### end

### grep_regex_question_extended
# Zero or one with -E
printf 'ac\nabc\nabbc\n' | grep -E 'ab?c'
### expect
ac
abc
### end

### grep_regex_alternation
# Alternation with -E
printf 'cat\ndog\nbird\n' | grep -E 'cat|dog'
### expect
cat
dog
### end

### grep_regex_group
# Grouping with -E
printf 'ab\naba\nabab\n' | grep -E '(ab)+'
### expect
ab
aba
abab
### end

### grep_character_class
# Character class
printf 'a1\nb2\nc3\n' | grep '[0-9]'
### expect
a1
b2
c3
### end

### grep_negated_class
# Negated character class
printf 'a1\n12\nb2\n' | grep '^[^0-9]'
### expect
a1
b2
### end

### grep_word_boundary_extended
### skip: word boundary in ERE not implemented
printf 'foo\nfoobar\nbar foo baz\n' | grep -E '\bfoo\b'
### expect
foo
bar foo baz
### end

### grep_dot_metachar
# Dot matches any char
printf 'cat\ncar\ncab\n' | grep 'ca.'
### expect
cat
car
cab
### end

### grep_escape_special
# Escape special chars with -F
printf 'a.b\na*b\na+b\n' | grep -F 'a.b'
### expect
a.b
### end

### grep_empty_pattern
### skip: empty pattern handling differs
printf 'foo\nbar\n' | grep ''
### expect
foo
bar
### end

### grep_whole_line
### skip: -x whole line match not implemented
printf 'foo\nfoobar\nbar foo\n' | grep -x foo
### expect
foo
### end

### grep_byte_offset
### skip: -b byte offset not implemented
printf 'foo\nbar\n' | grep -b bar
### expect
4:bar
### end

### grep_show_filename
### skip: filename display not implemented for stdin
printf 'foo\n' | grep -H foo
### expect
(standard input):foo
### end

### grep_no_filename
# No filename prefix
printf 'foo\n' | grep -h foo
### expect
foo
### end

### grep_color
### skip: --color not implemented
printf 'foo\n' | grep --color=always foo
### expect
foo
### end

### grep_perl_regex
### skip: -P perl regex not implemented
printf 'foo123\n' | grep -P 'foo\d+'
### expect
foo123
### end

### grep_ignore_binary
### skip: binary file detection not implemented
printf 'foo\0bar\n' | grep foo
### expect
foo
### end

### grep_include_pattern
### skip: --include not implemented
grep --include='*.txt' pattern /some/dir
### exit_code: 1
### expect
### end

### grep_exclude_pattern
### skip: --exclude not implemented
grep --exclude='*.log' pattern /some/dir
### exit_code: 1
### expect
### end

### grep_line_buffered
### skip: --line-buffered not implemented
printf 'foo\nbar\n' | grep --line-buffered foo
### expect
foo
### end

### grep_ignore_case_extended
# Case insensitive with extended regex
printf 'Hello\nWORLD\nhello\n' | grep -iE 'hello'
### expect
Hello
hello
### end

### grep_regex_range
# Character range
printf 'a\nb\nc\nd\n' | grep '[a-c]'
### expect
a
b
c
### end

### grep_empty_input
# Empty input
printf '' | grep foo
### exit_code: 1
### expect
### end

### grep_binary_files_text
### skip: -a flag not implemented
printf 'foo\0bar\n' | grep -a foo
### expect
foo
### end

### grep_count_multiple_matches
# Count with multiple lines
printf 'foo\nfoo\nbar\nfoo\n' | grep -c foo
### expect
3
### end

### grep_invert_count
# Count inverted matches
printf 'foo\nbar\nbaz\n' | grep -vc foo
### expect
2
### end

### grep_combined_flags
# Multiple flags combined
printf 'FOO\nfoo\nfoobar\n' | grep -ic foo
### expect
3
### end

### grep_regex_quantifier_extended
# Exact quantifier with -E
printf 'a\naa\naaa\naaaa\n' | grep -E 'a{2,3}'
### expect
aa
aaa
aaaa
### end

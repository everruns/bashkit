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
# Quiet mode - no output, just exit status
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
# Stop after N matches
printf 'foo\nfoo\nfoo\n' | grep -m 2 foo
### expect
foo
foo
### end

### grep_after_context
# Show N lines after match
printf 'a\nfoo\nb\nc\n' | grep -A 1 foo
### expect
foo
b
### end

### grep_before_context
# Show N lines before match
printf 'a\nb\nfoo\nc\n' | grep -B 1 foo
### expect
b
foo
### end

### grep_context
# Show N lines before and after match
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
# Multiple -e patterns (OR matching)
printf 'foo\nbar\nbaz\n' | grep -e foo -e bar
### expect
foo
bar
### end

### grep_pattern_file
### skip: requires file redirection support
printf 'foo\nbar\nbaz\n' | grep -f /patterns.txt
### expect
foo
bar
### end

### grep_null_data
# Null-terminated mode with -z
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
# Word boundary in extended regex
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
# Empty pattern matches all lines
printf 'foo\nbar\n' | grep ''
### expect
foo
bar
### end

### grep_whole_line
# Match whole line only
printf 'foo\nfoobar\nbar foo\n' | grep -x foo
### expect
foo
### end

### grep_byte_offset
# Show byte offset
printf 'foo\nbar\n' | grep -b bar
### expect
4:bar
### end

### grep_show_filename
# Show filename for stdin with -H
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
# Color flag accepted (no-op, outputs plain text)
printf 'foo\n' | grep --color=always foo
### expect
foo
### end

### grep_perl_regex
# Perl regex support (uses regex crate which supports PCRE features)
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
# Include only .txt files in recursive search
mkdir -p /tmp/grepdir && printf 'hello\n' > /tmp/grepdir/a.txt && printf 'hello\n' > /tmp/grepdir/b.log && grep -r --include='*.txt' hello /tmp/grepdir
### expect
/tmp/grepdir/a.txt:hello
### end

### grep_exclude_pattern
# Exclude .log files in recursive search
mkdir -p /tmp/grepdir2 && printf 'hello\n' > /tmp/grepdir2/a.txt && printf 'hello\n' > /tmp/grepdir2/b.log && grep -r --exclude='*.log' hello /tmp/grepdir2
### expect
/tmp/grepdir2/a.txt:hello
### end

### grep_include_no_match
# Include pattern that matches no files
mkdir -p /tmp/grepdir3 && printf 'hello\n' > /tmp/grepdir3/a.log && grep -r --include='*.txt' hello /tmp/grepdir3
echo $?
### expect
1
### end

### grep_line_buffered
# Line-buffered flag accepted (no-op, output is already line-oriented)
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
# Treat binary as text with -a (filters null bytes)
printf 'foo\0bar\n' | grep -a foo
### expect
foobar
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

### grep_max_count_exact
# Max count equals matches
printf 'foo\nfoo\n' | grep -m 2 foo
### expect
foo
foo
### end

### grep_max_count_one
# Max count of 1
printf 'foo\nfoo\nfoo\n' | grep -m 1 foo
### expect
foo
### end

### grep_context_multiple_matches
# Context with multiple matches close together
printf 'a\nfoo\nb\nfoo\nc\n' | grep -A 1 foo
### expect
foo
b
foo
c
### end

### grep_context_overlapping
# Overlapping context regions should merge
printf 'a\nfoo\nb\nbar\nc\n' | grep -C 1 -e foo -e bar
### expect
a
foo
b
bar
c
### end

### grep_whole_line_no_match
# Whole line with partial match should not match
printf 'foobar\n' | grep -x foo
### exit_code: 1
### expect
### end

### grep_whole_line_with_spaces
# Whole line including spaces
printf 'foo bar\nfoo\nbar\n' | grep -x 'foo bar'
### expect
foo bar
### end

### grep_multiple_patterns_no_match
# Multiple patterns with no match
printf 'baz\nqux\n' | grep -e foo -e bar
### exit_code: 1
### expect
### end

### grep_quiet_with_count
# Quiet should override count
printf 'foo\nfoo\n' | grep -qc foo
### exit_code: 0
### expect
### end

### grep_max_count_with_context
# Max count with context
printf 'a\nfoo\nb\nfoo\nc\nfoo\nd\n' | grep -m 2 -A 1 foo
### expect
foo
b
foo
c
### end

### grep_before_context_at_start
# Before context at file start
printf 'foo\nb\nc\n' | grep -B 2 foo
### expect
foo
### end

### grep_after_context_at_end
# After context at file end
printf 'a\nb\nfoo\n' | grep -A 2 foo
### expect
foo
### end

### grep_context_with_line_numbers
# Context with line numbers
printf 'a\nfoo\nb\n' | grep -n -C 1 foo
### expect
1-a
2:foo
3-b
### end

### grep_multiple_patterns_case_insensitive
# Multiple patterns with case insensitive
printf 'FOO\nBAR\nbaz\n' | grep -i -e foo -e bar
### expect
FOO
BAR
### end

### grep_whole_line_case_insensitive
# Whole line with case insensitive
printf 'FOO\nfoo\nFOObar\n' | grep -ix foo
### expect
FOO
foo
### end

### grep_max_count_zero
# Max count of zero should match nothing
printf 'foo\nfoo\n' | grep -m 0 foo
### exit_code: 1
### expect
### end

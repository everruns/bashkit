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

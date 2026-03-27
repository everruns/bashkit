### rg_basic_match
# Basic rg pattern match
printf 'hello world\ngoodbye\nhello again\n' | rg hello
### expect
hello world
hello again
### end

### rg_no_line_numbers_default
# rg suppresses line numbers when piped (non-tty)
printf 'foo\nbar\nbaz\n' | rg bar
### expect
bar
### end

### rg_line_numbers_with_n
# -n flag enables line numbers
printf 'foo\nbar\nbaz\n' | rg -n bar
### expect
2:bar
### end

### rg_no_line_number_N_flag
# -N flag explicitly suppresses line numbers
printf 'foo\nbar\n' | rg -N bar
### expect
bar
### end

### rg_no_line_number_long
# --no-line-number long flag
printf 'foo\nbar\n' | rg --no-line-number bar
### expect
bar
### end

### rg_line_number_long
# --line-number long flag enables line numbers
printf 'foo\nbar\nbaz\n' | rg --line-number bar
### expect
2:bar
### end

### rg_no_match
# No match returns exit code 1
printf 'foo\nbar\n' | rg xyz
### exit_code: 1
### expect
### end

### rg_case_insensitive
# Case insensitive search
printf 'Hello\nWORLD\nhello\n' | rg -i hello
### expect
Hello
hello
### end

### rg_count
# Count matches
printf 'foo\nbar\nfoo again\n' | rg -c foo
### expect
2
### end

### rg_invert_match
# Invert match
printf 'foo\nbar\nbaz\n' | rg -v foo
### expect
bar
baz
### end

### rg_fixed_strings
# Fixed string (no regex)
printf 'a.b\naxb\n' | rg -F 'a.b'
### expect
a.b
### end

### rg_word_boundary
# Word boundary match
printf 'cat\ncatch\nmy cat\n' | rg -w cat
### expect
cat
my cat
### end

### rg_max_count
# Stop after N matches
printf 'foo\nfoo\nfoo\n' | rg -m 1 foo
### expect
foo
### end

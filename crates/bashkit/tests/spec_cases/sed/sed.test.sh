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
### skip: multiple commands not fully implemented
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
### skip: append command not implemented
printf 'one\ntwo\n' | sed '/one/a\inserted'
### expect
one
inserted
two
### end

### sed_insert
### skip: insert command not implemented
printf 'one\ntwo\n' | sed '/two/i\inserted'
### expect
one
inserted
two
### end

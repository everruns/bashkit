# Quoting tests
# Inspired by Oils spec/quote.test.sh
# https://github.com/oilshell/oil/blob/master/spec/quote.test.sh

### quote_unquoted_words
# Unquoted words collapse whitespace
echo unquoted    words
### expect
unquoted words
### end

### quote_single_quoted
# Single-quoted preserves whitespace
echo 'single   quoted'
### expect
single   quoted
### end

### quote_two_single_parts
# Two single-quoted parts join
### skip: TODO adjacent single-quoted strings not joined correctly
echo 'two single-quoted pa''rts in one token'
### expect
two single-quoted parts in one token
### end

### quote_unquoted_and_single
# Unquoted and single-quoted join
echo unquoted' and single-quoted'
### expect
unquoted and single-quoted
### end

### quote_newline_in_single
# newline inside single-quoted string
echo 'newline
inside single-quoted string'
### expect
newline
inside single-quoted string
### end

### quote_double_quoted
# Double-quoted preserves whitespace
echo "double   quoted"
### expect
double   quoted
### end

### quote_mix_in_one_word
# Mix of quotes in one word
echo unquoted'  single-quoted'"  double-quoted  "unquoted
### expect
unquoted  single-quoted  double-quoted  unquoted
### end

### quote_var_sub
# Var substitution in double quotes
FOO=bar
echo "==$FOO=="
### expect
==bar==
### end

### quote_var_sub_braces
# Var substitution with braces
FOO=bar
echo foo${FOO}
### expect
foobar
### end

### quote_var_sub_braces_quoted
# Var substitution with braces, quoted
FOO=bar
echo "foo${FOO}"
### expect
foobar
### end

### quote_var_length
# Var length in double quotes
FOO=bar
echo "foo${#FOO}"
### expect
foo3
### end

### quote_backslash_store_echo
# Storing backslashes and then echoing them
one='\'
two='\\'
echo "$one" "$two"
### expect
\ \\
### end

### quote_backslash_escapes
# Backslash escapes outside quotes
echo \$ \| \a \b \c \d \\
### expect
$ | a b c d \
### end

### quote_backslash_in_double
# Backslash escapes inside double quoted string
echo "\$ \\ \\ \p \q"
### expect
$ \ \ \p \q
### end

### quote_no_c_escape_in_double
# C-style backslash escapes NOT special in double quotes
echo "\a \b"
### expect
\a \b
### end

### quote_literal_dollar
# Literal $
echo $
### expect
$
### end

### quote_quoted_literal_dollar
# Quoted literal $
echo $ "$" $
### expect
$ $ $
### end

### quote_line_continuation
# Line continuation
echo foo\
$
### expect
foo$
### end

### quote_line_continuation_double
# Line continuation inside double quotes
echo "foo\
$"
### expect
foo$
### end

### quote_semicolon
# Semicolon separates commands
echo separated; echo by semi-colon
### expect
separated
by semi-colon
### end

### quote_no_tab_in_single
# No tab escapes within single quotes
echo 'a\tb'
### expect
a\tb
### end

### quote_dollar_single_basic
# $'' basic
echo $'foo'
### expect
foo
### end

### quote_dollar_single_quotes
# $'' with quotes
echo $'single \' double \"'
### expect
single ' double "
### end

### quote_dollar_single_newlines
# $'' with newlines
echo $'col1\ncol2\ncol3'
### expect
col1
col2
col3
### end

### quote_dollar_double_synonym
# $"" is a synonym for ""
echo $"foo"
x=x
echo $"foo $x"
### expect
foo
foo x
### end

### quote_dollar_single_hex
# $'' with hex escapes
echo $'\x41\x42\x43'
### expect
ABC
### end

### quote_dollar_single_octal
# $'' with octal escapes
echo $'\101\102\103'
### expect
ABC
### end

### quote_dollar_single_unicode_u
# $'' with \u unicode escape
echo $'\u0041\u0042'
### expect
AB
### end

### quote_dollar_single_unicode_U
# $'' with \U unicode escape
echo $'\U00000041\U00000042'
### expect
AB
### end

### quote_dollar_single_special
# $'' with special escapes
printf '%s' $'\a' | od -A n -t x1 | tr -d ' \n'
echo
printf '%s' $'\b' | od -A n -t x1 | tr -d ' \n'
echo
printf '%s' $'\t' | od -A n -t x1 | tr -d ' \n'
echo
printf '%s' $'\n' | od -A n -t x1 | tr -d ' \n'
echo
### expect
07
08
09
0a
### end

### quote_empty_string_preserved
# Empty string as argument is preserved when quoted
### skip: TODO set -- with quoted empty args not preserving count
set -- "" "a" ""
echo $#
### expect
3
### end

### quote_nested_quotes_in_command_sub
# Nested quotes in command substitution
echo "$(echo "hello world")"
### expect
hello world
### end

### quote_backslash_newline_removed
# Backslash-newline is line continuation (removed)
echo he\
llo
### expect
hello
### end

### quote_single_quote_in_double
# Single quote inside double quotes
echo "it's"
### expect
it's
### end

### quote_double_in_single
# Double quote inside single quotes
echo 'say "hi"'
### expect
say "hi"
### end

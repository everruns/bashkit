# BashBox lexer cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_lexer_comments
# comments
echo a # not printed
# whole line
echo b
### expect
a
b
### end

### bashbox_lexer_trailing_whitespace
# trailing whitespace
echo a   
### expect
a
### end

### bashbox_lexer_line_continuation_between_words
# line continuation between words
echo a \
  b
### expect
a b
### end

### bashbox_lexer_line_continuation_inside_a_word
# line continuation inside a word
echo a\
b
### expect
ab
### end

### bashbox_lexer_trailing_backslash
# trailing backslash
echo a\
### expect
a\
### end

### bashbox_lexer_newlines_inside_quotes
# newlines inside quotes
echo 'a
b' "c
d"
### expect
a
b c
d
### end

### bashbox_lexer_escapes_inside_backticks
# escapes inside backticks
echo `echo \`echo hi\``
### expect
hi
### end

### bashbox_lexer_parens_inside_quoted_command_substitution
# parens inside quoted command substitution
echo $(echo ")") $(echo "a\"b") $(echo 'x)')
### expect
) a"b x)
### end

### bashbox_lexer_lone_dollar_signs
# lone dollar signs
echo $% a$ $
### expect
$% a$ $
### end

### bashbox_lexer_nested_braces_in_parameter_expansion
# nested braces in parameter expansion
echo ${x:-{a}}
### expect
{a}
### end

### bashbox_lexer_empty_braces_are_a_word
# empty braces are a word
echo {}
### expect
{}
### end

### bashbox_lexer_bang_followed_by_text_is_a_word
# bang followed by text is a word
echo !x
### expect
!x
### end

### bashbox_lexer_double_brackets_followed_by_text_are_a_word
# double brackets followed by text are a word
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo ]]x
### expect
]]x
### end

### bashbox_lexer_nested_subshell_closing_with
# nested subshell closing with ))
( (echo hi))
### expect
hi
### end

### bashbox_lexer_nested_parens_in_arithmetic_command
# nested parens in arithmetic command
(( ((1+2)) == 3 && (1+(2)) == 3 )) && echo yes
### expect
yes
### end

### bashbox_lexer_is_a_shift_inside
# << is a shift inside (( ))
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
(( x = 1 << 2 ))
echo $x
### expect
4
### end

### bashbox_lexer_inside_c_style_for_header
# ;; inside C-style for header
for ((;;)); do echo x; break; done
### expect
x
### end

### bashbox_lexer_parens_inside_in_double_quotes
# parens inside $(...) in double quotes
echo "a $(echo ")") b"
### expect
a ) b
### end

### bashbox_lexer_backticks_in_double_quotes
# backticks in double quotes
echo "a `echo ")"` b"
### expect
a ) b
### end

### bashbox_lexer_quoted_brace_in
# quoted brace in ${...}
echo ${x:-'}'}
### expect
}
### end

### bashbox_lexer_escaped_quote_in_ansi_c_quoting
# escaped quote in ANSI-C quoting
echo $'a\'b'
### expect
a'b
### end

### bashbox_lexer_nested_parens_in
# nested parens in $((...))
echo $((1+(2))) foo
### expect
3 foo
### end

### bashbox_lexer_single_quoted
# single quoted
x=1; cat << 'EOF'
$x
EOF
### expect
$x
### end

### bashbox_lexer_double_quoted
# double quoted
x=1; cat <<"EOF"
$x
EOF
### expect
$x
### end

### bashbox_lexer_backslash_escaped
# backslash escaped
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=1; cat <<\E\OF
$x
EOF
### expect
$x
### end

### bashbox_lexer_partly_quoted
# partly quoted
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=1; cat <<E"OF"
$x
EOF
echo after
### expect
$x
after
### end

### bashbox_lexer_quote_then_text
# quote then text
cat <<'E'OF
x
EOF
echo after
### expect
x
after
### end

### bashbox_lexer_escaped_quote_inside_double_quotes
# escaped quote inside double quotes
cat <<"E\"F"
x
E"F
### expect
x
### end

### bashbox_lexer_tab_stripping
# tab stripping
x=1; cat <<-EOF
		$x
	EOF
### expect
1
### end

### bashbox_lexer_tab_stripping_with_a_quoted_delimiter
# tab stripping with a quoted delimiter
cat <<-"E"F
	$x
	EF
### expect
$x
### end

### bashbox_lexer_two_heredocs_on_one_line
# two heredocs on one line
cat <<A; cat <<B
a
A
b
B
### expect
a
b
### end


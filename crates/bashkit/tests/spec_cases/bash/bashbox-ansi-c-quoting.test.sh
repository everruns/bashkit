# BashBox ansi-c-quoting cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_ansi_c_quoting_the_result_is_quoted_text
# the result is quoted text
x=$'a\nb'; echo "$x"; echo "$'q'" '$'"'q'"; echo $"dq $x"; echo $'it\'s' $'a''b' x$'\t'y; touch zz; echo $'z'* $'*'
### expect
a
b
$'q' $'q'
dq a
b
it's ab x	y
zz *
### end

### bashbox_ansi_c_quoting_type_shows_it_single_quoted
# type shows it single-quoted
f() { echo $'a\tb' $'it\'s' "$'x'" $"y"; }
type f
### expect
f is a function
f () 
{ 
    echo 'a	b' 'it'\''s' "$'x'" "y"
}
### end

### bashbox_ansi_c_quoting_here_documents_expand_as_in_double_quotes_quotes_stay
# here-documents expand as in double quotes, quotes stay
x=1; cat <<EOF
don't "q" $x \$x $'a' `echo bt` "\"" \\
 a\
b
EOF
echo "`echo in dq` \` \a"
### expect
don't "q" 1 $x $'a' bt "\"" \
 ab
in dq ` \a
### end

# A backslash outside quotes makes the next character literal: `\$` is a
# dollar sign, not the start of a parameter or command substitution.

### escaped_dollar_not_expanded
HOME=/h
echo \$HOME
echo a\$HOME
echo \${HOME}
x=\$HOME; echo "$x"
### expect
$HOME
a$HOME
${HOME}
$HOME
### end

### escaped_dollar_paren_not_substituted
echo \$\(echo hi\)
echo a\$\(b\)
### expect
$(echo hi)
a$(b)
### end

### escaped_backtick_literal
echo \`echo hi\`
### expect
`echo hi`
### end

### escaped_dollar_printf_args
printf '%s\n' a\$1 \$PATH
### expect
a$1
$PATH
### end

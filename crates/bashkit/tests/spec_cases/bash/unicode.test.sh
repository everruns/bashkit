# Unicode tests
# Inspired by Oils spec/unicode.test.sh
# https://github.com/oilshell/oil/blob/master/spec/unicode.test.sh

### unicode_echo_literal
# Unicode literal in echo
echo μ
### expect
μ
### end

### unicode_single_quoted
# Unicode in single quotes
echo 'μ'
### expect
μ
### end

### unicode_double_quoted
# Unicode in double quotes
echo "μ"
### expect
μ
### end

### unicode_dollar_single
# Unicode in $'' via \u escape
### bash_diff: system bash may not support \u in $''
echo $'\u03bc'
### expect
μ
### end

### unicode_dollar_single_U
# Unicode in $'' via \U escape
### bash_diff: system bash may not support \U in $''
echo $'\U000003bc'
### expect
μ
### end

### unicode_printf_u
# printf \u escape
### bash_diff: system bash printf \u requires UTF-8 locale
printf '\u03bc\n'
### expect
μ
### end

### unicode_printf_U
# printf \U escape
### bash_diff: system bash printf \U requires UTF-8 locale
printf '\U000003bc\n'
### expect
μ
### end

### unicode_var_with_unicode
# Variable with unicode value
x=café
echo $x
### expect
café
### end

### unicode_string_length
# String length of unicode string
### bash_diff: system bash ${#x} counts bytes in POSIX locale, chars in UTF-8
x=café
echo ${#x}
### expect
4
### end

### unicode_in_array
# Unicode strings in array
arr=(α β γ)
echo ${arr[0]} ${arr[1]} ${arr[2]}
echo ${#arr[@]}
### expect
α β γ
3
### end

### unicode_in_case
# Unicode in case pattern
x=α
case $x in
  α) echo matched ;;
  *) echo no ;;
esac
### expect
matched
### end

### unicode_in_test
# Unicode in test/comparison
x=café
if [[ $x == café ]]; then echo equal; fi
### expect
equal
### end

### unicode_concatenation
# Unicode string concatenation
a="hello "
b="世界"
echo "$a$b"
### expect
hello 世界
### end

### unicode_in_for
# Unicode in for loop
for c in α β γ; do
  printf "[%s]" "$c"
done
echo
### expect
[α][β][γ]
### end

### unicode_multibyte_echo
# Multi-byte unicode characters
echo "日本語"
### expect
日本語
### end

### unicode_emoji
# Emoji in strings
echo "hello 🌍"
### expect
hello 🌍
### end

### unicode_dollar_single_ascii
# $'' with unicode for ASCII range
### bash_diff: system bash may not support \u in $''
echo $'\u0041\u0042\u0043'
### expect
ABC
### end

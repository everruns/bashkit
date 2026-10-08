### length_counts_bytes_under_c_locale
s="ação"
LC_ALL=C
echo "${#s}"
LC_ALL=C.UTF-8
echo "${#s}"
### expect
6
4
### end

### length_c_locale_from_lc_ctype_and_lang
s="日本"
LC_ALL=
LC_CTYPE=POSIX
echo "${#s}"
LC_CTYPE=C.UTF-8
echo "${#s}"
LC_CTYPE=
LANG=C
echo "${#s}"
### expect
6
2
6
### end

### length_c_locale_array_element_and_arithmetic
LC_ALL=C
a=(ação)
s=ñ
echo "${#a[0]} $(( ${#s} + 0 ))"
### expect
6 2
### end

### length_c_locale_scoped_by_local
s="ação"
LC_ALL=C
f() { local LC_ALL=C.UTF-8; echo "in=${#s}"; }
f
echo "out=${#s}"
### expect
in=4
out=6
### end

### length_inside_default_operand_counts_characters
s="ação"
echo "${unset_x:-${#s}}"
LC_ALL=C
echo "${unset_x:-${#s}}"
### expect
4
6
### end

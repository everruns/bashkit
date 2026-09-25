### awk_fs_regex_plus
# -F with a multi-char value is an ERE (issue #2445)
printf 'a  b\n' | awk -F' +' '{print NF}'
### expect
2
### end

### awk_fs_regex_bracket
# -F'[ ,]' splits on either space or comma
printf 'a,b c\n' | awk -F'[ ,]' '{print NF; print $3}'
### expect
3
c
### end

### awk_fs_regex_variable
# FS assigned in BEGIN is also an ERE
printf 'x1y22z\n' | awk 'BEGIN { FS = "[0-9]+" } {print NF, $1, $2, $3}'
### expect
3 x y z
### end

### awk_fs_regex_leading_separator
# Regex FS keeps a leading empty field (unlike the default FS)
printf ',,a,b\n' | awk -F',+' '{print NF; print "[" $1 "]"}'
### expect
3
[]
### end

### awk_fs_single_char_is_literal
# A single-character FS is literal even if it is a regex metacharacter
printf 'a|b|c\n' | awk -F'|' '{print NF, $2}'
printf 'a.b.c\n' | awk -F. '{print NF, $3}'
### expect
3 b
3 c
### end

### awk_fs_tab
# -F'\t' splits on tabs only
printf 'a b\tc\n' | awk -F'\t' '{print NF; print $1}'
### expect
2
a b
### end

### awk_fs_empty_line
# An empty record has zero fields with a non-default FS
printf '\n' | awk -F, '{print NF}'
### expect
0
### end

### awk_split_regex_literal
# split() with a regex constant separator
echo a1b2c | awk '{n = split($0, p, /[0-9]/); print n, p[1], p[2], p[3]}'
### expect
3 a b c
### end

### awk_split_regex_string
# split() with a multi-char string separator treats it as an ERE
echo 'a--b-c' | awk '{n = split($0, p, "-+"); print n, p[3]}'
### expect
3 c
### end

### awk_split_single_char_literal
# split() with a single-char string separator is literal
echo 'a.b.c' | awk '{n = split($0, p, "."); print n, p[2]}'
### expect
3 b
### end

### awk_split_default_fs_regex
# split() without a separator uses FS as a regex
echo 'a  b   c' | awk -F' +' '{n = split($0, p); print n, p[3]}'
### expect
3 c
### end

### awk_split_clears_array
# split() deletes previous array contents
awk 'BEGIN { split("a b c", p); n = split("x", p); print n, length(p), (3 in p) }'
### expect
1 1 0
### end

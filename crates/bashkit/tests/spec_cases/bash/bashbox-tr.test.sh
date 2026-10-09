# BashBox tr cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_tr_ranges
# ranges
echo hello | tr a-z A-Z
### expect
HELLO
### end

### bashbox_tr_short_set2_repeats_its_last_char
# short set2 repeats its last char
echo hello | tr a-z A-C
### expect
CCCCC
### end

### bashbox_tr_octal_escape
# octal escape
echo ABC | tr '\101' x
### expect
xBC
### end

### bashbox_tr_octal_escape_stops_after_three_digits
# octal escape stops after three digits
echo A8 | tr '\1018' xy
### expect
xy
### end

### bashbox_tr_other_escaped_char_is_literal
# other escaped char is literal
echo a-b | tr 'a\-' xy
### expect
xyb
### end

### bashbox_tr_trailing_dash_is_literal
# trailing dash is literal
echo a-b | tr 'a-' xy
### expect
xyb
### end

### bashbox_tr_class_keeps_a_following
# class keeps a following ]
echo 'a]b' | tr '[:lower:]]' X
### expect
XXX
### end

### bashbox_tr_delete
# delete
echo hello world | tr -d lo
### expect
he wrd
### end

### bashbox_tr_squeeze
# squeeze
echo 'aa  bb' | tr -s '[:blank:]'
### expect
aa bb
### end

### bashbox_tr_squeeze_range
# squeeze range
echo aabbccdd | tr -s a-c
### expect
abcdd
### end

### bashbox_tr_translate_then_squeeze_set2
# translate then squeeze set2
echo 'a..b,,c' | tr -s ., __
### expect
a_b_c
### end

### bashbox_tr_delete_then_squeeze
# delete then squeeze
echo 'xaxxbx  c' | tr -ds x ' '
### expect
ab c
### end

### bashbox_tr_space_class_in_byte_order
# space class in byte order
printf '\t\n\v\f\r ' | tr '[:space:]' abcdef; echo; printf 'aA0 \t' | tr '[:alnum:][:blank:]' 'a-zA-Z0-9xy'; echo
### expect
abcdef
Kkayx
### end

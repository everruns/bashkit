# BashBox options cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_options_grep_o_is_an_option
# grep -o is an option
echo abc | grep -o b
### expect
b
### end

### bashbox_options_grep_long_options
# grep long options
echo ABC | grep --ignore-case --only-matching b; echo x | grep --regexp=x
### expect
B
x
### end

### bashbox_options_tr_complements_set1
# tr complements set1
echo abc | tr -c a X; echo aabbc | tr -cs a X; echo aabbc | tr --complement -s a
### expect
aXXXaaXaabc
### end

### bashbox_options_tr_truncates_set1
# tr truncates set1
echo abcd | tr -t abcd xy
### expect
xycd
### end

### bashbox_options_wc_m_counts_characters_as_bytes
# wc -m counts characters as bytes
printf 'a b\n' | wc -m; printf 'a b\n' | wc --chars --lines
### expect
4
      1       4
### end

### bashbox_options_cut_long_options
# cut long options
echo a:b | cut --delimiter=: --fields=2 --complement
### expect
a
### end

### bashbox_options_cut_s_drops_lines_without_a_delimiter
# cut -s drops lines without a delimiter
printf 'a\tb\nc\n' | cut -sf1
### expect
a
### end

### bashbox_options_cut_takes_an_empty_delimiter_as_nul
# cut takes an empty delimiter as NUL
printf 'a\0b\n' | cut -d '' -f2
### expect
b
### end

### bashbox_options_tail_bytes
# tail --bytes
printf 'a\nb\n' | tail --bytes=2
### expect
b
### end

### bashbox_options_base64_wrapping
# base64 wrapping
printf hello | base64 -w0; echo; printf hello | base64 --wrap=4
### expect
aGVsbG8=
aGVs
bG8=
### end

### bashbox_options_date_long_options
# date long options
date --utc --date=@0 +%F; date --universal -d @86400 +%F
### expect
1970-01-01
1970-01-02
### end

### bashbox_options_file_commands_long_options
# file commands long options
mkdir --parents p/q; touch --no-create p/none; echo x | tee --append p/q/f >/dev/null; rm --recursive --force p/none; cp --recursive p r; ls r/q
### expect
f
### end

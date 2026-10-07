# BashBox fold cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_fold_default_width_80
# default width 80
printf '%0100d\n' 0 | fold
### expect
00000000000000000000000000000000000000000000000000000000000000000000000000000000
00000000000000000000
### end

### bashbox_fold_spaces_long_word
# spaces long word
printf 'abcdefghijklmnop qr\n' | fold -s -w 5
### expect
abcde
fghij
klmno
p qr
### end

### bashbox_fold_backspace_at_column_0
# backspace at column 0
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '\x08abcde\n' | fold -w 3
### expect
abc
de
### end

### bashbox_fold_tab_wider_than_width
# tab wider than width
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '\tab\n' | fold -w 4
### expect
	
ab
### end

### bashbox_fold_space_overflows_the_line
# space overflows the line
printf 'abcde fgh\n' | fold -s -w 5
### expect
abcde
 fgh
### end

### bashbox_fold_line_ending_in_a_blank
# line ending in a blank
printf 'ab cd ef gh ij\n' | fold -s -w 3
### expect
ab 
cd 
ef 
gh 
ij
### end

### bashbox_fold_tab_after_break_with_s
# tab after break with -s
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'ab\tcdefghij\n' | fold -s -w 6
### expect
ab
	
cdefgh
ij
### end

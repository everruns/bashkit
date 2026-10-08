# BashBox stat cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_stat_name_needing_double_quotes
# name needing double quotes
printf x > "a'b c"; stat -c %N "a'b c"
### expect
"a'b c"
### end

### bashbox_stat_name_needing_escapes
# name needing escapes
### skip: L-FS-004: file names cannot hold control characters, so $'a\nb' is rejected (TM-DOS-015)
printf x > $'a\nb'; stat -c %N $'a\nb'
### expect
'a'$'\n''b'
### end

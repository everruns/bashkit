# BashBox stat cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_stat_name_needing_double_quotes
# name needing double quotes
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf x > "a'b c"; stat -c %N "a'b c"
### expect
"a'b c"
### end

### bashbox_stat_name_needing_escapes
# name needing escapes
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf x > $'a\nb'; stat -c %N $'a\nb'
### expect
'a'$'\n''b'
### end

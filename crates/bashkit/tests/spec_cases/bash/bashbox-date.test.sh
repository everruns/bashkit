# BashBox date cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_date_default_format
# default format
date -u -d @0
### expect
Thu Jan  1 00:00:00 UTC 1970
### end

### bashbox_date_combined_ud
# combined -ud
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
date -ud @86400; date -ud@86400 +%F
### expect
Fri Jan  2 00:00:00 UTC 1970
1970-01-02
### end

### bashbox_date_parses_date_strings
# parses date strings
date -u -d '2024-02-29 13:05:09' '+%j %H:%M:%S'
### expect
060 13:05:09
### end

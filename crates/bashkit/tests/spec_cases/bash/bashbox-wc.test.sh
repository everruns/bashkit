# BashBox wc cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_wc_stdin_reserves_seven_columns
# stdin reserves seven columns
echo hi | wc; echo hi | wc -lc
### expect
      1       1       3
      1       3
### end

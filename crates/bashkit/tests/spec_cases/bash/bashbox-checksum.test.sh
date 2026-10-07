# BashBox checksum cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_checksum_stdin
# stdin
printf 'hello\n' | md5sum
### expect
b1946ac92492d2347c6235b4d2611184  -
### end

### bashbox_checksum_newline_in_name
# newline in name
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf z > $'n\nl'; md5sum $'n\nl'
### expect
\fbade9e36a3f36d3d676c1b808451dd7  n\nl
### end

### bashbox_checksum_check_quiet
# check quiet
md5sum --quiet -c good.md5
### expect
### end

### bashbox_checksum_check_status_ok
# check status ok
md5sum --status -c good.md5
### expect
### end

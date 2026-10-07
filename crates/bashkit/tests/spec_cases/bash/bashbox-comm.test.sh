# BashBox comm cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_comm_suppress_all
# suppress all
comm -123 a b
### expect
### end

### bashbox_comm_zero_terminated
# zero terminated
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\0b\0' > z1; printf 'b\0c\0' > z2; comm -z z1 z2 | od -c
### expect
0000000   a  \0  \t  \t   b  \0  \t   c  \0
0000011
### end

### bashbox_comm_empty_files
# empty files
printf '' > e; comm e e
### expect
### end

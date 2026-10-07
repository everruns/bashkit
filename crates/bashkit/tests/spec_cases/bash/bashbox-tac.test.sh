# BashBox tac cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_tac_reads_stdin
# reads stdin
printf '1\n2\n' | tac
### expect
2
1
### end

### bashbox_tac_empty_separator_is_nul
# empty separator is NUL
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\0b\0' | tac -s '' | od -c
### expect
0000000   b  \0   a  \0
0000004
### end

### bashbox_tac_empty_input
# empty input
printf '' | tac
### expect
### end

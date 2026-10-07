# BashBox paste cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_paste_stdin_twice_takes_turns
# stdin twice takes turns
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '1\n2\n3\n4\n5\n' | paste - -
### expect
1	2
3	4
5	
### end

### bashbox_paste_serial_stdin_twice
# serial stdin twice
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '1\n2\n' | paste -s - -
### expect
1	2

### end

### bashbox_paste_zero_terminated
# zero terminated
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\0b\0' | paste -z - - | od -c
### expect
0000000   a  \t   b  \0
0000004
### end

### bashbox_paste_serial_zero_terminated
# serial zero terminated
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\0b' | paste -sz - | od -c
### expect
0000000   a  \t   b  \0
0000004
### end

### bashbox_paste_empty_files
# empty files
paste e e
### expect
### end

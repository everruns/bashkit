# BashBox od cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_od_hexl_trailer_partial
# hexl trailer partial
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'abc' | od -t x2z
### expect
0000000 6261 0063                                >abc<
0000003
### end

### bashbox_od_empty_input_no_address
# empty input no address
od -An e
### expect
### end

### bashbox_od_stdin
# stdin
printf 'hi' | od -c
### expect
0000000   h   i
0000002
### end

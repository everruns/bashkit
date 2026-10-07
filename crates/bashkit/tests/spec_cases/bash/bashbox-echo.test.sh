# BashBox echo cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_echo_joins_arguments
# joins arguments
echo a  b
### expect
a b
### end

### bashbox_echo_escapes_are_literal_by_default
# escapes are literal by default
echo 'a\tb'
### expect
a\tb
### end

### bashbox_echo_e_expands_escapes
# -e expands escapes
echo -e 'a\tb\\c\n'
### expect
a	b\c

### end

### bashbox_echo_e_unknown_escape_and_trailing_backslash_stay
# -e unknown escape and trailing backslash stay
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo -e '\z\xg\'
### expect
\z\xg\
### end

### bashbox_echo_e_hex
# -e hex
echo -e '\x41\x4a2'
### expect
AJ2
### end

### bashbox_echo_e_c_stops_output_and_the_newline
# -e \c stops output and the newline
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo -e 'a\cb'; echo -e x
### expect
ax
### end

### bashbox_echo_last_of_e_e_wins
# last of -e/-E wins
echo -eE 'a\tb'; echo -Ee 'a\tb'
### expect
a\tb
a	b
### end

### bashbox_echo_options_stop_at_the_first_non_option
# options stop at the first non-option
echo -nx; echo -- -n; echo -; echo a -n
### expect
-nx
-- -n
-
a -n
### end

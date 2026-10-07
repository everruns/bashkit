# BashBox expand cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_expand_past_the_last_stop_is_one_space
# past the last stop is one space
printf 'a\tb\tc\td\n' | expand -t 2,4
### expect
a b c d
### end

### bashbox_expand_extend
# extend
printf 'a\tb\tc\td\te\n' | expand -t 2,5,/3
### expect
a b  c   d  e
### end

### bashbox_expand_increment
# increment
printf 'a\tb\tc\td\te\n' | expand -t 2,5,+3
### expect
a b  c  d  e
### end

### bashbox_expand_only_extend
# only extend
printf 'a\tb\n' | expand -t /3
### expect
a  b
### end

### bashbox_expand_only_increment
# only increment
printf 'a\tb\n' | expand -t +3
### expect
a  b
### end

### bashbox_expand_repeated_t_adds_stops
# repeated -t adds stops
printf 'a\tb\tc\n' | expand -t 2 -t 6
### expect
a b   c
### end

### bashbox_expand_stdin
# stdin
printf '\tz\n' | expand -t 3 -
### expect
   z
### end

### bashbox_expand_slash_then_value_later
# slash then value later
printf 'a\tb\tc\n' | expand -t /,4
### expect
a   b   c
### end

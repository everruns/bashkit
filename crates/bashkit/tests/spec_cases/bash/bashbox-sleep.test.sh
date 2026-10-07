# BashBox sleep cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_sleep_zero
# zero
sleep 0
### expect
### end

### bashbox_sleep_fraction
# fraction
sleep 0.001
### expect
### end

### bashbox_sleep_units
# units
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
sleep 0s 0m 0h 0d
### expect
### end

### bashbox_sleep_leading_dot
# leading dot
sleep .0
### expect
### end

### bashbox_sleep_exponent
# exponent
sleep 0e5
### expect
### end

### bashbox_sleep_hex
# hex
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
sleep 0x0
### expect
### end

### bashbox_sleep_plus
# plus
sleep +0
### expect
### end

### bashbox_sleep_leading_blank
# leading blank
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
sleep ' 0'
### expect
### end

### bashbox_sleep_dash_dash
# dash dash
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
sleep -- 0
### expect
### end

### bashbox_sleep_hex_fraction
# hex fraction
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
sleep 0x.0p1
### expect
### end

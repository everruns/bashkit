# BashBox seq cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_seq_counting_down
# counting down
seq 5 -2 1
### expect
5
3
1
### end

### bashbox_seq_negative_operands_are_not_options
# negative operands are not options
seq -3 -1
### expect
-3
-2
-1
### end

### bashbox_seq_empty_range_prints_nothing
# empty range prints nothing
seq 3 1; seq -s, 3 1
### expect
### end

### bashbox_seq_decimals_follow_first_and_increment
# decimals follow FIRST and INCREMENT
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
seq 1 0.5 2; seq 1 2.50; seq 1.0 2
### expect
1.0
1.5
2.0
1
2
1.0
2.0
### end

### bashbox_seq_custom_formats
# custom formats
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
seq -f '%03g' 9 10; seq -f '%.2f' 1 0.25 1.5; seq -f '%e' 1
### expect
009
010
1.00
1.25
1.50
1.000000e+00
### end

### bashbox_seq_separator
# separator
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
seq -s, 1 3; seq --separator=-1 1 2
### expect
1,2,3
1-12
### end

### bashbox_seq_equal_width_pads_after_the_sign
# equal width pads after the sign
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
seq -w 1 -0.5 -1; seq -w 1 50 100; seq --equal-width 9 10
### expect
01.0
00.5
00.0
-0.5
-1.0
001
051
09
10
### end

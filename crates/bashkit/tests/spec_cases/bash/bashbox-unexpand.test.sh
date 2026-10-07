# BashBox unexpand cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_unexpand_single_space_before_stop
# single space before stop
printf 'abcdefg h\n' | unexpand -a
### expect
abcdefg h
### end

### bashbox_unexpand_single_blank_becomes_tab
# single blank becomes tab
printf 'abcdefg \tx\n' | unexpand -a
### expect
abcdefg		x
### end

### bashbox_unexpand_list
# list
printf '  a   b      c\n' | unexpand -t 2,6
### expect
	a	b      c
### end

### bashbox_unexpand_list_exhausted
# list exhausted
printf '  a   b      c   d\n' | unexpand -t 2,6
### expect
	a	b      c   d
### end

### bashbox_unexpand_extend
# extend
printf '  a  b  c  d\n' | unexpand -t 2,/3
### expect
	a  b  c  d
### end

### bashbox_unexpand_increment
# increment
printf '  a  b  c  d\n' | unexpand -t 2,+3
### expect
	a	b	c	d
### end

### bashbox_unexpand_obsolete_list
# obsolete list
printf '  a   b      c\n' | unexpand -2,6
### expect
	a   b      c
### end

### bashbox_unexpand_obsolete_trailing_comma
# obsolete trailing comma
printf '  a   b      c\n' | unexpand -2,6,
### expect
	a   b      c
### end

### bashbox_unexpand_trailing_blanks_without_newline
# trailing blanks without newline
printf 'a       ' | unexpand -a | od -c
### expect
0000000   a  \t
0000002
### end

### bashbox_unexpand_non_blank_stops_leading_conversion
# non-blank stops leading conversion
printf 'x       y\n' | unexpand
### expect
x       y
### end

### bashbox_unexpand_tab_at_stop
# tab at stop
printf '\t\t  a\n' | unexpand -t 4
### expect
		  a
### end

### bashbox_unexpand_spaces_then_tab
# spaces then tab
printf '   \ta\n' | unexpand
### expect
	a
### end

### bashbox_unexpand_one_space_then_tab
# one space then tab
printf 'abcdefg \t \tx\n' | unexpand -a
### expect
abcdefg			x
### end

### bashbox_unexpand_blank_past_last_stop
# blank past last stop
printf 'abc     d\n' | unexpand -t 2
### expect
abc			d
### end

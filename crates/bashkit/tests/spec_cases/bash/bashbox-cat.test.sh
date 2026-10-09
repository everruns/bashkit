# BashBox cat cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_cat_b_numbers_only_non_blank_lines_and_wins_over_n
# -b numbers only non-blank lines and wins over -n
printf 'a\n\nb\n' | cat -nb
### expect
     1	a

     2	b
### end

### bashbox_cat_s_squeezes_blank_runs_across_files
# -s squeezes blank runs across files
printf 'a\n\n' > c1; printf '\n\nb\n' > c2; cat -sn c1 c2
### expect
     1	a
     2	
     3	b
### end

### bashbox_cat_t_shows_tabs
# -T shows tabs
printf 'a\tb\n' | cat -T
### expect
a^Ib
### end

### bashbox_cat_v_shows_control_and_high_bytes
# -v shows control and high bytes
printf 'a\t\001\177\200\211\240\377\n' | cat -v
### expect
a	^A^?M-^@M-^IM- M-^?
### end

### bashbox_cat_a_is_vet
# -A is -vET
printf 'a\tb\r\n' | cat -A
### expect
a^Ib^M$
### end

### bashbox_cat_e_is_ve
# -e is -vE
printf '\ta\r\n' | cat -e
### expect
	a^M$
### end

### bashbox_cat_t_is_vt
# -t is -vT
printf '\ta\r\n' | cat -t
### expect
^Ia^M
### end

### bashbox_cat_e_marks_line_ends_and_shows_a_cr_before_them
# -E marks line ends and shows a CR right before them
printf 'x\r\ny\r\n\r\n' | cat -E
### expect
x^M$
y^M$
^M$
### end

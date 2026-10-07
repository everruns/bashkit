# BashBox printf cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_printf_string_flags_width_and_precision
# string flags, width and precision
printf '%s|%5s|%-5s|%.2s|%%\n' a b c xyz
### expect
a|    b|c    |xy|%
### end

### bashbox_printf_integers
# integers
printf '%d %i %+d %05d %o %x %X %u\n' 7 -3 5 42 8 255 255 9
### expect
7 -3 +5 00042 10 ff FF 9
### end

### bashbox_printf_hex_and_octal_integer_arguments
# hex and octal integer arguments
printf '%d %d %d\n' 0x1F 010 ' 5'
### expect
31 8 5
### end

### bashbox_printf_floats
# floats
printf '%f|%.1f|%05.1f|%F\n' 1e2 2.25 3.14159 1
### expect
100.000000|2.2|003.1|1.000000
### end

### bashbox_printf_char_takes_the_first_character
# char takes the first character
printf '%c%c\n' hello w
### expect
hw
### end

### bashbox_printf_format_is_reused_for_leftover_arguments
# format is reused for leftover arguments
printf '%s-%s\n' a b c
### expect
a-b
c-
### end

### bashbox_printf_format_without_conversions_is_printed_once
# format without conversions is printed once
printf 'plain\n' extra args
### expect
plain
### end

### bashbox_printf_format_escapes
# format escapes
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '\101|\0101|\x41|\e|\t|\\|\q|\c\n'
### expect
A|1|A||	|\|\q|\c
### end

### bashbox_printf_b_expands_escapes_in_the_argument
# %b expands escapes in the argument
printf '%b|%b\n' 'a\tb' '\0101\101'
### expect
a	b|AA
### end

### bashbox_printf_e_and_e
# %e and %E
printf '[%e|%E|%.0e|%#.0e|%10.3e|%+.3e|%e|%.10e]\n' 100 0.000123 12345 5 1234.5678 0 1e100 1
### expect
[1.000000e+02|1.230000E-04|1e+04|5.e+00| 1.235e+03|+0.000e+00|1.000000e+100|1.0000000000e+00]
### end

### bashbox_printf_g_picks_e_or_f_and_drops_trailing_zeros
# %g picks %e or %f and drops trailing zeros
printf '[%g|%G|%g|%g|%#g|%g|%.3g|%g|%.0g|%#.3g|%G|%g]\n' 100000 1e-5 1234567 0.0001 1 0 3.14159 -2.5e300 15 1 1e20 0.00001234
### expect
[100000|1E-05|1.23457e+06|0.0001|1.00000|0|3.14|-2.5e+300|2e+01|1.00|1E+20|1.234e-05]
### end

### bashbox_printf_f_flags
# %f flags
printf '[%f|%.0f|%#.0f|%10.3f|%-10.2f|%+f|% f|%f|%010.2f]\n' 1e2 2.5 3 3.14159 2.5 1 1 -0 -3.14159
### expect
[100.000000|2|3.|     3.142|2.50      |+1.000000| 1.000000|-0.000000|-000003.14]
### end

### bashbox_printf_inf_and_nan
# inf and nan
printf '[%f|%F|%e|%g|%G|%5f|%-6e|%06f]\n' inf -inf nan INFINITY nan inf -inf inf
### expect
[inf|-INF|nan|inf|NAN|  inf|-inf  |   inf]
### end

### bashbox_printf_the_flag_on_integers
# the # flag on integers
printf '[%#x|%#X|%#o|%#o|%#x|%-#8x|%#08x|%#.0o]\n' 255 255 8 0 0 255 255 0
### expect
[0xff|0XFF|010|0|0|0xff    |0x0000ff|0]
### end

### bashbox_printf_sign_and_precision_flags_on_integers
# sign and precision flags on integers
printf '[% d|% d|%+ d|%08.3d|%.0d|%-05d|%.3x]\n' 5 -5 5 7 0 3 255
### expect
[ 5|-5|+5|     007||3    |0ff]
### end

### bashbox_printf_star_width_and_precision
# star width and precision
printf '[%*d|%-*d|%.*f|%*.*s|%*d]\n' 5 1 5 2 2 3.14159 6 2 abcdef -4 9
### expect
[    1|2    |3.14|    ab|9   ]
### end

### bashbox_printf_length_modifiers_are_ignored
# length modifiers are ignored
printf '[%ld|%lld|%hhx|%Lf|%zd|%jd|%td|%qd]\n' 1 2 255 1.5 3 4 6 5
### expect
[1|2|ff|1.500000|3|4|6|5d]
### end

### bashbox_printf_the_grouping_flag_is_a_no_op_in_the_c_locale
# the grouping flag is a no-op in the C locale
printf "[%'d]\n" 1234567
### expect
[1234567]
### end

### bashbox_printf_bash_pads_b_and_q_itself_never_with_zeros
# bash pads %b and %q itself, never with zeros
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '[%05b|%05q|%05Q]\n' 'a\tb' 'c d' e
### expect
[  a	b| c\ d|    e]
### end

### bashbox_printf_q_and_q_with_width_and_precision
# %q and %Q with width and precision
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '[%10q|%-6q|%.2q|%.2Q|%5Q]\n' 'a b' x abc 'a bc' 'a b'
### expect
[      a\ b|x     |ab|a\ | a\ b]
### end

### bashbox_printf_fmt_t_formats_epoch_seconds
# %(fmt)T formats epoch seconds
printf '[%(%Y-%m-%d)T|%12(%m)T|%-6.2(%Y)T]\n' 2980800 2980800 0
### expect
[1970-02-04|          02|19    ]
### end

### bashbox_printf_integer_argument_forms
# integer argument forms
printf '%d %d %d %d %d %d %i %X\n' "'A" '"B' '+0x10' '-010' "'" ' 12' 0x7f 3054
### expect
65 66 16 -8 0 12 127 BEE
### end

### bashbox_printf_ends_options
# -- ends options
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf -- '[%s]\n' a
### expect
[a]
### end

### bashbox_printf_after_the_format_is_an_argument
# -- after the format is an argument
printf '[%s]\n' -- a
### expect
[--]
[a]
### end

### bashbox_printf_v_assigns_through_the_shell
# -v assigns through the shell
printf -v x %s a; echo "$x"
### expect
a
### end

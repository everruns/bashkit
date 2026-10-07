# BashBox xargs cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_xargs_defaults_to_echo
# defaults to echo
printf 'a b\n c' | xargs
### expect
a b c
### end

### bashbox_xargs_appends_items_to_the_command
# appends items to the command
echo a b | xargs echo X
### expect
X a b
### end

### bashbox_xargs_n_limits_items_per_run
# -n limits items per run
echo a b c | xargs -n 2; echo a b | xargs -n1 echo -
### expect
a b
c
- a
- b
### end

### bashbox_xargs_options_after_the_command_belong_to_it
# options after the command belong to it
echo a b | xargs -n1 echo -n; echo
### expect
ab
### end

### bashbox_xargs_i_runs_once_per_line_keeping_trailing_blanks
# -I runs once per line, keeping trailing blanks
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'one\n\n  two  \n' | xargs -I {} echo '<{}>' {}
### expect
<one> one
<two  > two  
### end

### bashbox_xargs_i_with_an_attached_placeholder
# -I with an attached placeholder
echo x | xargs -I% echo [%]
### expect
[x]
### end

### bashbox_xargs_runs_once_on_empty_input
# runs once on empty input
printf '' | xargs echo hi
### expect
hi
### end

### bashbox_xargs_r_skips_empty_input
# -r skips empty input
printf '' | xargs -r echo hi
### expect
### end

### bashbox_xargs_quotes_and_backslashes_group_words
# quotes and backslashes group words
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '%s\n' '"a b" '\''c d'\'' e\\ f g\\\\h' 'i\\' 'j' | xargs -n1 echo
### expect
a b
c d
e\
f
g\\h
i\
j
### end

### bashbox_xargs_an_empty_quoted_word_is_kept_but_a_final_one_is_dropped
# an empty quoted word is kept but a final one is dropped
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '"" a\nb ""' | xargs -n1 echo -; printf "c '" | xargs echo
### expect
- 
- a
- b
c
### end

### bashbox_xargs_a_backslash_escapes_a_newline
# a backslash escapes a newline
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\\\nb\n' | xargs -n1 echo; printf 'c\\' | xargs echo
### expect
a
b
c
### end

### bashbox_xargs_a_nul_ends_an_argument_with_a_warning
# a NUL ends an argument, with a warning
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\0b c\0d\n' | xargs echo
### expect
a c
### end

### bashbox_xargs_0_splits_at_nuls
# -0 splits at NULs
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a b\0\0c\0' | xargs -0 -n1 echo -
### expect
- a b
- 
- c
### end

### bashbox_xargs_d_takes_a_character_or_an_escape
# -d takes a character or an escape
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a,b,,c' | xargs -d, echo; printf 'a\tb\n' | xargs -d '\t' echo; printf 'aXb' | xargs -d '\x58' echo; printf 'aXb' | xargs --delimiter='\130' echo; printf 'a\ab' | xargs -d '\a' echo
### expect
a b  c
a b

a b
a b
a b
### end

### bashbox_xargs_d_numbers_are_read_like_strtoul
# -d numbers are read like strtoul
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'aAb' | xargs -d '\x 41' echo; printf 'aAb' | xargs -d '\x+0x41' echo; printf 'a\0b' | xargs -d '\x' echo; printf 'a\0b' | xargs -d '\x-0' echo
### expect
a b
a b
a b
a b
### end

### bashbox_xargs_d_items_keep_blanks_and_quotes
# -d items keep blanks and quotes
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf "a 'b\nc\n" | xargs -d '\n' -n1 echo
### expect
a 'b
c
### end

### bashbox_xargs_e_stops_at_the_end_of_file_word
# -E stops at the end-of-file word
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a b\nSTOP\nc\n' | xargs -E STOP echo; printf 'a STOP b\n' | xargs -eSTOP echo; printf 'STOP\nc\n' | xargs --eof=STOP echo x; printf 'a STOP' | xargs -E STOP; printf 'STOP' | xargs -E STOP echo y
### expect
a b
a
x
a STOP
y
### end

### bashbox_xargs_e_on_a_line_ending
# -E on a line ending
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a STOP\nb\n' | xargs -E STOP -L1 echo
### expect
a
### end

### bashbox_xargs_a_bare_e_turns_e_off
# a bare -e turns -E off
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo a x b | xargs -E x -e echo; echo a x b | xargs -E x --eof echo
### expect
a x b
a x b
### end

### bashbox_xargs_e_has_no_effect_with_0_or_d
# -E has no effect with -0 or -d
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\0x\0b' | xargs -E x -0 echo
### expect
a x b
### end

### bashbox_xargs_l_runs_per_lines
# -L runs per lines
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\nb\nc\nd\ne\n' | xargs -L2 echo
### expect
a b
c d
e
### end

### bashbox_xargs_l_continues_a_line_ending_in_a_blank
# -L continues a line ending in a blank
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a b \nc\nd\\ \ne\n' | xargs -L1 echo
### expect
a b c
d  e
### end

### bashbox_xargs_l_defaults_to_one_line
# -l defaults to one line
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\nb\nc\n' | xargs -l echo; printf 'a\nb\nc\n' | xargs -l2 echo; printf 'a\nb\n' | xargs --max-lines echo
### expect
a
b
c
a b
c
a
b
### end

### bashbox_xargs_a_bare_optional_option_at_the_end_of_a_bundle
# a bare optional option at the end of a bundle
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo a x b | xargs -E x -tl echo; echo y | xargs -ti echo {}; echo z | xargs -it echo {}
### expect
a
y
{}
### end

### bashbox_xargs_l_n_and_i_exclude_each_other
# -L, -n and -I exclude each other
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\nb\nc\n' | xargs -n2 -L1 echo; printf 'a\nb\nc\n' | xargs -L1 -n2 echo; printf 'a\nb\n' | xargs -I{} -l echo {}
### expect
a
b
c
a b
c
{} a
{} b
### end

### bashbox_xargs_i_excludes_n_and_l
# -I excludes -n and -L
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\nb\n' | xargs -L1 -I{} echo {}; printf 'a\nb\n' | xargs -n2 -I{} echo {}; printf 'a\nb\n' | xargs -I{} -n2 echo {}
### expect
a
b
a
b
{} a b
### end

### bashbox_xargs_n1_after_i_is_ignored
# -n1 after -I is ignored
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a b\nc\n' | xargs -I{} -n1 echo [{}]
### expect
[a b]
[c]
### end

### bashbox_xargs_i_defaults_to
# -i defaults to {}
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\nb\n' | xargs -i echo {}; echo c | xargs --replace echo {}; echo d | xargs -iX echo X
### expect
a
b
c
d
### end

### bashbox_xargs_numbers_may_have_blanks_and_a_sign
# numbers may have blanks and a sign
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo a b c | xargs -n ' +2' echo; echo d | xargs -P 2 echo
### expect
a b
c
d
### end

### bashbox_xargs_s_limits_the_command_length
# -s limits the command length
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\nb\nc\n' | xargs -s 9 echo
### expect
a b
c
### end

### bashbox_xargs_x_alone_still_splits
# -x alone still splits
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a b c d\n' | xargs -x -s 9 echo
### expect
a b
c d
### end

### bashbox_xargs_i_size_limits
# -I size limits
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'abc\n' | xargs -I{} -s 8 echo {} {}; printf 'abcde\n' | xargs -I{} -s 6 echo x{}; printf 'abcdef\n' | xargs -I{} -s 6 echo x{}; printf 'a\n' | xargs -I{} -s 3 echo {}; printf '' | xargs -I{} -s 3 echo {}
### expect
### end

### bashbox_xargs_i_with_0
# -I with -0
printf 'a b\0c\0' | xargs -0 -I{} echo [{}]
### expect
[a b]
[c]
### end

### bashbox_xargs_a_reads_standard_input
# -a - reads standard input
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\nb\n' | xargs -a - echo
### expect
a b
### end

### bashbox_xargs_a_with_a_directory_reads_nothing
# -a with a directory reads nothing
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
mkdir d; xargs -a d echo x
### expect
x
### end

### bashbox_xargs_t_prints_each_command
# -t prints each command
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a b\nc\n' | xargs -t -n2 echo; printf "it's\ta b\n" | xargs -d '\n' --verbose
### expect
a b
c
it's	a b
### end

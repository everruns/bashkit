# BashBox sort cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_sort_bytewise
# bytewise
printf 'b\nB\na\n10\n9\n' | sort
### expect
10
9
B
a
b
### end

### bashbox_sort_empty_input
# empty input
printf '' | sort
### expect
### end

### bashbox_sort_missing_final_newline_is_added
# missing final newline is added
printf 'b\na' | sort
### expect
a
b
### end

### bashbox_sort_reverse
# reverse
printf 'a\nc\nb\n' | sort -r
### expect
c
b
a
### end

### bashbox_sort_numeric_with_ties_broken_bytewise
# numeric with ties broken bytewise
printf 'b\na\n10\n9\n-1\n+5\n.5\n' | sort -n
### expect
-1
+5
a
b
.5
9
10
### end

### bashbox_sort_unique
# unique
printf 'b\na\nb\n' | sort -u
### expect
a
b
### end

### bashbox_sort_numeric_unique_keeps_the_first_of_a_run
# numeric unique keeps the first of a run
printf '1\n01\nb\na\n' | sort -nu
### expect
b
1
### end

### bashbox_sort_reverse_unique
# reverse unique
printf 'B\na\nb\nA\na\n' | sort -ur
### expect
b
a
B
A
### end

### bashbox_sort_key_to_end_of_line_ties_by_whole_line
# key to end of line, ties by whole line
printf 'b 1\na 1\n' | sort -k2
### expect
a 1
b 1
### end

### bashbox_sort_key_keeps_leading_blanks
# key keeps leading blanks
printf 'x  b\ny a\n' | sort -k2
### expect
x  b
y a
### end

### bashbox_sort_key_past_last_field_is_empty
# key past last field is empty
printf 'b a\na b\n' | sort -k3
### expect
a b
b a
### end

### bashbox_sort_numeric_key_range
# numeric key range
printf 'a 2 z\nb 10 a\nc 2 b\n' | sort -k2,2n
### expect
a 2 z
c 2 b
b 10 a
### end

### bashbox_sort_key_modifiers_override_global_r
# key modifiers override global -r
printf 'a 2 z\nb 10 a\nc 2 b\n' | sort -r -k2n
### expect
c 2 b
a 2 z
b 10 a
### end

### bashbox_sort_global_options_apply_to_a_plain_key
# global options apply to a plain key
printf 'a 2\nb 10\nc 3\n' | sort -rn -k2
### expect
b 10
c 3
a 2
### end

### bashbox_sort_delimiter
# delimiter
printf 'a:3\nb:1\nc:2\n' | sort -t: -k2
### expect
b:1
c:2
a:3
### end

### bashbox_sort_unique_by_key
# unique by key
printf 'a b c\nz b d\na c b\n' | sort -u -k2,2
### expect
a b c
a c b
### end

### bashbox_sort_multiple_keys
# multiple keys
printf 'b 2\na 2\nc 1\na 1\n' | sort -k2,2 -k1,1r
### expect
c 1
a 1
b 2
a 2
### end

### bashbox_sort_character_positions_in_keys
# character positions in keys
printf 'xb2\nya1\nzb1\n' | sort -k1.2,1.2 -k1.3n
### expect
ya1
zb1
xb2
### end

### bashbox_sort_character_position_past_the_field_end
# character position past the field end
printf 'ab c\nab a\na b\n' | sort -k1.3
### expect
ab a
ab c
a b
### end

### bashbox_sort_end_character_position
# end character position
printf 'abc 1\nabd 0\nabb 2\n' | sort -k1,1.2 -k2n
### expect
abd 0
abc 1
abb 2
### end

### bashbox_sort_end_position_before_start_is_an_empty_key
# end position before start is an empty key
printf 'b\na\nc\n' | sort -k2,1
### expect
a
b
c
### end

### bashbox_sort_b_on_the_start_position_skips_blanks
# b on the start position skips blanks
printf 'x   b\ny a\nz  c\n' | sort -k2b
### expect
y a
x   b
z  c
### end

### bashbox_sort_b_on_the_end_position
# b on the end position
printf 'a  xy\nb zz\nc  xa\n' | sort -k2,2.2b
### expect
c  xa
a  xy
b zz
### end

### bashbox_sort_global_b_applies_to_keys_without_modifiers
# global -b applies to keys without modifiers
printf 'x   b\ny a\nz  c\n' | sort -b -k2
### expect
y a
x   b
z  c
### end

### bashbox_sort_key_modifiers_stop_global_ones_being_inherited
# key modifiers stop global ones being inherited
printf 'x   b\ny a\nz  c\n' | sort -b -k2r
### expect
y a
z  c
x   b
### end

### bashbox_sort_t_splits_on_every_separator
# -t splits on every separator
printf 'a::3\nb:1:2\nc:2:1\n' | sort -t: -k2,2 -k3n
### expect
a::3
b:1:2
c:2:1
### end

### bashbox_sort_t_key_ending_on_a_whole_field
# -t key ending on a whole field
printf 'a:b:c\na:a:d\n' | sort -t: -k1,2
### expect
a:a:d
a:b:c
### end

### bashbox_sort_t_with_character_positions
# -t with character positions
printf 'k:xb\nj:ya\n' | sort -t: -k2.2
### expect
j:ya
k:xb
### end

### bashbox_sort_t_nul_separator
# -t NUL separator
printf 'b\000a\na\000b\n' | sort -t '\0' -k2 | tr '\0' '|'
### expect
b|a
a|b
### end

### bashbox_sort_d_dictionary_order
# -d dictionary order
printf 'a-c\nab\na c\n' | sort -d
### expect
a c
ab
a-c
### end

### bashbox_sort_f_folds_case
# -f folds case
printf 'b\nA\na\nB\n' | sort -f
### expect
A
a
B
b
### end

### bashbox_sort_fu_keeps_the_first_of_each_folded_run
# -fu keeps the first of each folded run
printf 'b\nA\na\nB\n' | sort -fu
### expect
A
b
### end

### bashbox_sort_i_ignores_nonprinting_characters
# -i ignores nonprinting characters
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'b\na\001c\nac\n' | sort -i | tr '\001' '^'
### expect
a^c
ac
b
### end

### bashbox_sort_m_month_order
# -M month order
printf 'mar x\n  feb\nJAN\nfoo\ndecember\n' | sort -M
### expect
foo
JAN
  feb
mar x
december
### end

### bashbox_sort_g_general_numeric
# -g general numeric
printf '1e3\n-inf\nnan\nabc\n10\n0x10\n-5.5\ninf\n+2\n' | sort -g
### expect
abc
nan
-inf
-5.5
+2
10
0x10
1e3
inf
### end

### bashbox_sort_h_human_numeric
# -h human numeric
printf '0K\n1\n-1K\n2M\n500K\n0\n1.5K\n3k\n1m\n-2\n' | sort -h
### expect
-1K
-2
0
0K
1
1m
1.5K
3k
500K
2M
### end

### bashbox_sort_n_with_signs_fractions_and_leading_zeros
# -n with signs, fractions and leading zeros
printf '007\n-0\n0.50\n.5\n-1.5\n-.5\n10\n1e5\n 3\n' | sort -n
### expect
-1.5
-.5
-0
.5
0.50
1e5
 3
007
10
### end

### bashbox_sort_n_compares_long_numbers_exactly
# -n compares long numbers exactly
printf '100000000000000000001\n100000000000000000000\n99999999999999999999\n' | sort -n
### expect
99999999999999999999
100000000000000000000
100000000000000000001
### end

### bashbox_sort_v_version_order
# -V version order
printf 'a-1.10\na-1.9\na-1.9~rc\n.b\n..\n.\nfile.tar.gz\nfile2.tar.gz\nfile10\nfile\n\nfile~\n.a\n1.0a\n1.0\n' | sort -V
### expect

.
..
.a
.b
1.0
1.0a
a-1.9~rc
a-1.9
a-1.10
file~
file
file.tar.gz
file2.tar.gz
file10
### end

### bashbox_sort_v_compares_suffixes_last
# -V compares suffixes last
printf 'foo.10.txt\nfoo.9.txt\nfoo.9.tar\nfoo.9.tar.gz\n' | sort -V
### expect
foo.9.tar
foo.9.tar.gz
foo.9.txt
foo.10.txt
### end

### bashbox_sort_s_keeps_input_order_for_equal_keys
# -s keeps input order for equal keys
printf 'b 1\na 1\nc 0\n' | sort -s -k2,2
### expect
c 0
b 1
a 1
### end

### bashbox_sort_s_without_keys_has_no_effect
# -s without keys has no effect
printf 'b\na\n' | sort -s
### expect
a
b
### end

### bashbox_sort_u_with_n_treats_equal_numbers_as_duplicates
# -u with -n treats equal numbers as duplicates
printf '1\n01\n1.0\n2\n' | sort -un
### expect
1
2
### end

### bashbox_sort_r_with_keys_reverses_the_last_resort_comparison
# -r with keys reverses the last-resort comparison
printf 'a 1\nb 1\n' | sort -r -k2n
### expect
b 1
a 1
### end

### bashbox_sort_z_uses_nul_line_endings
# -z uses NUL line endings
printf 'b\000a\000c' | sort -z | tr '\0' '\n'
### expect
a
b
c
### end

### bashbox_sort_o_writes_to_a_file_after_reading_all_input
# -o writes to a file after reading all input
printf 'b\na\n' > f; sort -o f f; cat f
### expect
a
b
### end

### bashbox_sort_c_on_sorted_input
# -c on sorted input
printf 'a\nb\nb\n' | sort -c
### expect
### end

### bashbox_sort_m_merges_sorted_inputs
# -m merges sorted inputs
printf 'a\nc\ne\n' > f1; printf 'b\nc\nd\n' > f2; sort -m f1 f2
### expect
a
b
c
c
d
e
### end

### bashbox_sort_m_does_not_sort
# -m does not sort
printf 'b\na\n' > f1; printf 'c\n' > f2; sort -m f1 f2
### expect
b
a
c
### end

### bashbox_sort_m_with_keys_and_reverse
# -m with keys and reverse
printf '3 a\n1 b\n' > f1; printf '2 c\n' > f2; sort -m -k1,1nr f1 f2
### expect
3 a
2 c
1 b
### end

### bashbox_sort_long_options
# long options
printf 'b:2\na:10\n' | sort --field-separator=: --key=2 --numeric-sort --reverse
### expect
a:10
b:2
### end

### bashbox_sort_sort_selects_the_comparison
# --sort selects the comparison
printf '10\n9\n' | sort --sort=num
### expect
9
10
### end

### bashbox_sort_ignored_performance_options
# ignored performance options
printf 'b\na\n' | sort -S 1M -T /tmp --parallel=2 --compress-program=gzip
### expect
a
b
### end

### bashbox_sort_r_groups_equal_lines
# -R groups equal lines
printf 'a\nb\na\nb\n' | sort -R | uniq | sort
### expect
a
b
### end

### bashbox_sort_d_is_compatible_with_v
# -d is compatible with -V
printf 'a-2\na-10\n' | sort -dV
### expect
a-2
a-10
### end

### bashbox_sort_r_with_f
# -R with -f
printf 'a\nA\nb\na\n' | sort -fR | tr A a | uniq | sort
### expect
a
b
### end

### bashbox_sort_the_same_tab_twice
# the same tab twice
printf 'b:1\na:2\n' | sort -t: -t: -k2
### expect
b:1
a:2
### end

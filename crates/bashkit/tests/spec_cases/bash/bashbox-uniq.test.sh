# BashBox uniq cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_uniq_c_counts_runs
# -c counts runs
printf 'a\na\nb\na\n' | uniq -c
### expect
      2 a
      1 b
      1 a
### end

### bashbox_uniq_d_keeps_only_repeated_lines
# -d keeps only repeated lines
printf 'a\na\nb\n' | uniq -d
### expect
a
### end

### bashbox_uniq_u_keeps_only_unique_lines
# -u keeps only unique lines
printf 'a\na\nb\n' | uniq -u
### expect
b
### end

### bashbox_uniq_adds_a_missing_final_newline
# adds a missing final newline
printf 'x\nx' | uniq
### expect
x
### end

### bashbox_uniq_empty_input
# empty input
printf '' | uniq
### expect
### end

### bashbox_uniq_second_operand_is_the_output_file
# second operand is the output file
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'a\na\n' > in; uniq in out; cat out
### expect
a
### end

### bashbox_uniq_i_ignores_case
# -i ignores case
printf 'a\nA\nb\n' | uniq -ic
### expect
      2 a
      1 b
### end

### bashbox_uniq_f_skips_fields
# -f skips fields
printf 'x a\ny a\nz b\n' | uniq -f1
### expect
x a
z b
### end

### bashbox_uniq_f_past_the_last_field_compares_empty_keys
# -f past the last field compares empty keys
printf 'a  b\nc b\n' | uniq -f 5
### expect
a  b
### end

### bashbox_uniq_s_skips_bytes
# -s skips bytes
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'xa\nya\nzb\n' | uniq --skip-chars=1
### expect
xa
zb
### end

### bashbox_uniq_w_compares_a_prefix
# -w compares a prefix
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf 'ab1\nab2\nac\n' | uniq -w2
### expect
ab1
ac
### end

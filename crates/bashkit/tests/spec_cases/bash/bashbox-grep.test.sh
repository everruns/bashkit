# BashBox grep cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_grep_stdin
# stdin
printf 'apple\nbanana\napricot' | grep ap
### expect
apple
apricot
### end

### bashbox_grep_h_names_stdin
# -H names stdin
echo apple | grep -Hc apple
### expect
(standard input):1
### end

### bashbox_grep_quiet
# quiet
grep -q cherry a.txt
### expect
### end

### bashbox_grep_quiet_stops_at_first_match
# quiet stops at first match
grep -q apple a.txt nope
### expect
### end

### bashbox_grep_only_matching_skips_empty_matches
# only matching skips empty matches
echo aaa | grep -o 'b*'
### expect
### end

### bashbox_grep_only_matching_with_v_prints_nothing
# only matching with -v prints nothing
grep -ov an a.txt
### expect
### end

### bashbox_grep_pattern_starting_with
# pattern starting with -
echo x-v | grep -e -v
### expect
x-v
### end

### bashbox_grep_newline_separates_patterns
# newline separates patterns
printf 'a\nb\nc\n' | grep "$(printf 'a\nc')"
### expect
a
c
### end

### bashbox_grep_fixed_strings
# fixed strings
printf 'a.c\nabc\n' | grep -F 'a.c'
### expect
a.c
### end

### bashbox_grep_word_match
# word match
printf 'pie\npies\n_pie\n' | grep -w pie
### expect
pie
### end

### bashbox_grep_word_match_with_alternation
# word match with alternation
printf 'ab\nxa\nb\n' | grep -wE 'a|b'
### expect
b
### end

### bashbox_grep_word_match_with_fixed_string
# word match with fixed string
printf 'foo.bar\nfoo.\n' | grep -wF 'foo.'
### expect
foo.
### end

### bashbox_grep_bre_is_literal
# BRE + is literal
printf 'a+b\nab\n' | grep 'a+b'
### expect
a+b
### end

### bashbox_grep_bre_alternation_and_groups
# BRE alternation and groups
printf 'cat\ndog\ncow\n' | grep '^\(cat\|dog\)$'
### expect
cat
dog
### end

### bashbox_grep_bre_back_reference
# BRE back-reference
printf 'abab\nabba\n' | grep '\(ab\)\1'
### expect
abab
### end

### bashbox_grep_bre_leading_star_is_literal
# BRE leading star is literal
printf '*a\nb\n' | grep '*a'
### expect
*a
### end

### bashbox_grep_ere
# ERE
printf 'aa\na\n' | grep -E '^a{2}$'
### expect
aa
### end

### bashbox_grep_g_resets_to_bre
# -G resets to BRE
printf 'a|b\na\n' | grep -G 'a|b'
### expect
a|b
### end

### bashbox_grep_slash_in_pattern
# slash in pattern
echo 'a/b' | grep 'a/b'
### expect
a/b
### end

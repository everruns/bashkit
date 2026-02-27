### wc_lines_only
# Count lines with -l
printf 'a\nb\nc\n' | wc -l
### expect
3
### end

### wc_words_only
# Count words with -w
printf 'one two three four five' | wc -w
### expect
5
### end

### wc_bytes_only
# Count bytes with -c
printf 'hello' | wc -c
### expect
5
### end

### wc_empty
# Empty input
printf '' | wc -l
### expect
0
### end

### wc_all_flags
# All counts (default)
printf 'hello world\n' | wc
### expect
      1       2      12
### end

### wc_multiple_lines
# Multiple lines
printf 'one\ntwo\nthree\n' | wc -l
### expect
3
### end

### wc_chars_m_flag
# Count characters with -m
printf 'hello' | wc -m
### expect
5
### end

### wc_lines_words
# Lines and words combined
printf 'one two\nthree four\n' | wc -lw
### expect
      2       4
### end

### wc_no_newline_at_end
# Input without trailing newline
printf 'hello world' | wc -w
### expect
2
### end

### wc_multiple_spaces
# Multiple spaces between words
printf 'hello   world' | wc -w
### expect
2
### end

### wc_tabs_count
# Tabs in input
printf 'a\tb\tc' | wc -w
### expect
3
### end

### wc_single_word
# Single word
printf 'word' | wc -w
### expect
1
### end

### wc_only_whitespace
# Only whitespace
printf '   \t   ' | wc -w
### expect
0
### end

### wc_max_line_length
printf 'short\nlongerline\n' | wc -L
### expect
10
### end

### wc_long_flags
# Long flag --lines
printf 'a\nb\n' | wc --lines
### expect
2
### end

### wc_long_words
# Long flag --words
printf 'one two three' | wc --words
### expect
3
### end

### wc_long_bytes
# Long flag --bytes
printf 'hello' | wc --bytes
### expect
5
### end

### wc_bytes_vs_chars
# Bytes vs chars for ASCII
printf 'hello' | wc -c && printf 'hello' | wc -m
### expect
5
5
### end

### wc_unicode_chars
### bash_diff: locale-dependent; real bash wc -m may count bytes in C locale
printf 'héllo' | wc -m
### expect
5
### end

### wc_unicode_bytes
# Unicode byte count
printf 'héllo' | wc -c
### expect
6
### end

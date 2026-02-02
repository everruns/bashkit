### wc_lines_only
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Count lines with -l
printf 'a\nb\nc\n' | wc -l
### expect
       3
### end

### wc_words_only
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Count words with -w
printf 'one two three four five' | wc -w
### expect
       5
### end

### wc_bytes_only
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Count bytes with -c
printf 'hello' | wc -c
### expect
       5
### end

### wc_empty
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Empty input
printf '' | wc -l
### expect
       0
### end

### wc_all_flags
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# All counts (default)
printf 'hello world\n' | wc
### expect
       1       2      12
### end

### wc_multiple_lines
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Multiple lines
printf 'one\ntwo\nthree\n' | wc -l
### expect
       3
### end

### wc_chars_m_flag
### skip: wc -m outputs full stats not just chars
# Count characters with -m
printf 'hello' | wc -m
### expect
       5
### end

### wc_lines_words
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Lines and words combined
printf 'one two\nthree four\n' | wc -lw
### expect
       2       4
### end

### wc_no_newline_at_end
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Input without trailing newline
printf 'hello world' | wc -w
### expect
       2
### end

### wc_multiple_spaces
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Multiple spaces between words
printf 'hello   world' | wc -w
### expect
       2
### end

### wc_tabs_count
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Tabs in input
printf 'a\tb\tc' | wc -w
### expect
       3
### end

### wc_single_word
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Single word
printf 'word' | wc -w
### expect
       1
### end

### wc_only_whitespace
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Only whitespace
printf '   \t   ' | wc -w
### expect
       0
### end

### wc_max_line_length
### skip: -L max line length not implemented
printf 'short\nlongerline\n' | wc -L
### expect
      10
### end

### wc_long_flags
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Long flag --lines
printf 'a\nb\n' | wc --lines
### expect
       2
### end

### wc_long_words
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Long flag --words
printf 'one two three' | wc --words
### expect
       3
### end

### wc_long_bytes
### skip: wc --bytes outputs full stats not just bytes
# Long flag --bytes
printf 'hello' | wc --bytes
### expect
       5
### end

### wc_bytes_vs_chars
### skip: wc -m outputs full stats not just chars
# Bytes vs chars for ASCII
printf 'hello' | wc -c && printf 'hello' | wc -m
### expect
       5
       5
### end

### wc_unicode_chars
### skip: unicode character counting not implemented
printf 'héllo' | wc -m
### expect
       5
### end

### wc_unicode_bytes
### bash_diff: BashKit wc uses fixed-width padding for stdin, real bash uses no padding
# Unicode byte count
printf 'héllo' | wc -c
### expect
       6
### end

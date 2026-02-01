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

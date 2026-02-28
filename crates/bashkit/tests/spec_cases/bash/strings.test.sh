### strings_text_extraction
# strings extracts printable text longer than minimum
printf 'hello world test data here\n' > /tmp/strings_text.txt
strings /tmp/strings_text.txt
### expect
hello world test data here
### end

### strings_min_length_flag
# strings -n sets minimum length
printf 'abcdefgh\n' > /tmp/strings_long.txt
strings -n 4 /tmp/strings_long.txt
### expect
abcdefgh
### end

### strings_short_string_filtered
# strings filters strings shorter than default min (4)
printf 'ab\n' > /tmp/strings_short.txt
strings /tmp/strings_short.txt | wc -l
### expect
0
### end

### strings_stdin
# strings reads from stdin
echo "test data from stdin" | strings -n 4
### expect
test data from stdin
### end

### strings_empty_file
# strings on empty file produces no output
touch /tmp/strings_empty.bin
strings /tmp/strings_empty.bin
### expect
### end

### strings_multiple_lines
# strings handles multiple lines of text
printf 'first line here\nsecond line here\n' > /tmp/strings_multi.txt
strings -n 4 /tmp/strings_multi.txt
### expect
first line here
second line here
### end

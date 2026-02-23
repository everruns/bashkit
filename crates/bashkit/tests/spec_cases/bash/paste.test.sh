### paste_stdin
# Paste stdin passthrough
printf 'a\nb\nc\n' | paste
### expect
a
b
c
### end

### paste_serial
# Serial mode merges lines with tabs
printf 'a\nb\nc\n' | paste -s
### expect
a	b	c
### end

### paste_custom_delimiter
# Custom delimiter with serial mode
printf '1\n2\n3\n' | paste -d, -s
### expect
1,2,3
### end

### paste_empty
# Empty input
printf '' | paste
echo done
### expect
done
### end

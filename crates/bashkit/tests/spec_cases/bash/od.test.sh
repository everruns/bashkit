### od_default_octal
# od defaults to octal byte dump from stdin
printf "AB" | od -An -tx1
### expect
 41 42
### end

### od_hex_short
# -tx1 is hex, -An suppresses the address column
printf "Hi" | od -An -tx1
### expect
 48 69
### end

### od_skip_bytes
# -j skips leading bytes
printf "ABCD" | od -An -j2 -tx1
### expect
 43 44
### end

### od_read_bytes
# -N caps the byte count
printf "ABCDE" | od -An -N3 -tx1
### expect
 41 42 43
### end

### od_width_columns
# -w controls bytes per line
printf "ABCD" | od -An -w2 -tx1
### expect
 41 42
 43 44
### end

### od_unknown_flag_rejected
### bash_diff: clap-backed od returns exit 2 for parse errors; GNU od returns 1
# Unknown long flag is a usage error
od --no-such-flag </dev/null 2>/dev/null; echo "exit=$?"
### expect
exit=2
### end

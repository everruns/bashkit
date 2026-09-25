### head_default
# Head outputs first 10 lines by default
printf '%s\n' 1 2 3 4 5 6 7 8 9 10 11 12 | head
### expect
1
2
3
4
5
6
7
8
9
10
### end

### head_n_flag
# Head with -n flag
printf 'a\nb\nc\nd\ne\n' | head -n 3
### expect
a
b
c
### end

### head_shorthand
# Head with -N shorthand
printf 'a\nb\nc\nd\ne\n' | head -2
### expect
a
b
### end

### tail_default
# Tail outputs last 10 lines by default
printf '%s\n' 1 2 3 4 5 6 7 8 9 10 11 12 | tail
### expect
3
4
5
6
7
8
9
10
11
12
### end

### tail_n_flag
# Tail with -n flag
printf 'a\nb\nc\nd\ne\n' | tail -n 3
### expect
c
d
e
### end

### tail_shorthand
# Tail with -N shorthand
printf 'a\nb\nc\nd\ne\n' | tail -2
### expect
d
e
### end

### head_fewer_lines
# Head when input has fewer lines than requested
printf 'a\nb\n' | head -n 10
### expect
a
b
### end

### tail_fewer_lines
# Tail when input has fewer lines than requested
printf 'a\nb\n' | tail -n 10
### expect
a
b
### end

### head_one_line
# Head with -n 1
printf 'first\nsecond\nthird\n' | head -n 1
### expect
first
### end

### tail_one_line
# Tail with -n 1
printf 'first\nsecond\nthird\n' | tail -n 1
### expect
third
### end

### head_empty_input
# Head with empty input
printf '' | head
echo done
### expect
done
### end

### tail_empty_input
# Tail with empty input
printf '' | tail
echo done
### expect
done
### end

### head_n_zero
# Head with -n 0 outputs nothing
printf 'a\nb\nc\n' | head -n 0
echo done
### expect
done
### end

### tail_n_zero
# Tail with -n 0 outputs nothing
printf 'a\nb\nc\n' | tail -n 0
echo done
### expect
done
### end

### head_n_negative
# head -n -N prints all but the last N lines (issue #2447)
printf '1\n2\n3\n' | head -n -1
### expect
1
2
### end

### head_n_negative_exceeds
# head -n -N with N >= line count prints nothing
printf '1\n2\n3\n' | head -n -5
echo done
### expect
done
### end

### head_n_negative_no_trailing_newline
# head -n -1 on input without trailing newline
printf '1\n2\n3' | head -n -1
### expect
1
2
### end

### head_c_negative
# head -c -N prints all but the last N bytes
printf 'abcdef' | head -c -2; echo
### expect
abcd
### end

### head_n_invalid
# head -n with non-numeric count is an error
printf 'a\n' | head -n abc
echo "rc=$?"
### expect
rc=1
### end

### head_no_trailing_newline_preserved
# head keeps a missing final newline as-is
printf 'a\nb' | head -n 5 | wc -c
### expect
3
### end

### head_n_suffix_k
# Counts accept multiplier suffixes (1k = 1024)
seq 2000 | head -n 1k | wc -l
### expect
1024
### end

### head_c_suffix_kb
# KB is a decimal multiplier (1000)
seq 5000 | head -c 2KB | wc -c
### expect
2000
### end

### head_invalid_suffix
# An unknown suffix is an invalid count
printf 'a\n' | head -n 3x
echo "rc=$?"
### expect
rc=1
### end

### head_missing_file_continues
# A missing file is reported, other files still print, exit 1
printf 'x\n' > /tmp/ht_present
head -n 1 /tmp/ht_missing /tmp/ht_present 2>/dev/null
echo "rc=$?"
### expect
==> /tmp/ht_present <==
x
rc=1
### end

### tail_n_negative_is_last
# tail -n -N is the same as tail -n N
printf '1\n2\n3\n' | tail -n -2
### expect
2
3
### end

### tail_c_last_bytes
# tail -c N prints the last N bytes
printf 'abcdef' | tail -c 2; echo
### expect
ef
### end

### tail_c_from_byte
# tail -c +N starts at byte N
printf 'abcdef' | tail -c +3; echo
### expect
cdef
### end

### tail_n_invalid
# tail rejects a non-numeric count
printf 'a\n' | tail -n abc
echo "rc=$?"
### expect
rc=1
### end

### tail_no_trailing_newline_preserved
# tail keeps a missing final newline as-is
printf 'a\nb' | tail -n 1 | wc -c
### expect
1
### end

### tail_stdin_dash_operand
# "-" reads standard input alongside files
printf 'f\n' > /tmp/ht_file
printf 's\n' | tail -n 1 - /tmp/ht_file
### expect
==> standard input <==
s

==> /tmp/ht_file <==
f
### end

### sort_basic
# Sort lines alphabetically
printf 'banana\napple\ncherry\n' | sort
### expect
apple
banana
cherry
### end

### sort_reverse
# Sort in reverse order
printf 'apple\nbanana\ncherry\n' | sort -r
### expect
cherry
banana
apple
### end

### sort_numeric
# Sort numerically
printf '10\n2\n1\n20\n' | sort -n
### expect
1
2
10
20
### end

### sort_unique
# Sort and remove duplicates
printf 'b\na\nb\nc\na\n' | sort -u
### expect
a
b
c
### end

### uniq_basic
# Remove adjacent duplicates
printf 'a\na\nb\nb\nb\nc\n' | uniq
### expect
a
b
c
### end

### uniq_count
# Count occurrences
printf 'a\na\nb\n' | uniq -c
### expect
      2 a
      1 b
### end

### sort_uniq_pipeline
# Common pattern: sort | uniq
printf 'c\na\nb\na\nc\na\n' | sort | uniq
### expect
a
b
c
### end

### sort_empty
# Sort empty input
printf '' | sort
echo done
### expect
done
### end

### uniq_empty
# Uniq empty input
printf '' | uniq
echo done
### expect
done
### end

### sort_single_line
# Sort single line
printf 'only\n' | sort
### expect
only
### end

### uniq_all_same
# All identical lines
printf 'same\nsame\nsame\n' | uniq
### expect
same
### end

### sort_numeric_mixed
# Numeric sort with mixed content
printf '5\n10\n2\n1\n' | sort -n
### expect
1
2
5
10
### end

### sort_reverse_numeric
# Reverse numeric sort
printf '1\n10\n2\n5\n' | sort -rn
### expect
10
5
2
1
### end

### sort_case_insensitive
printf 'Banana\napple\nCherry\n' | sort -f
### expect
apple
Banana
Cherry
### end

### sort_field_delim
# Sort by field with delimiter
printf 'b:2\na:1\nc:3\n' | sort -t: -k2n
### expect
a:1
b:2
c:3
### end

### sort_key_field
# Sort by key field
printf 'Bob 25\nAlice 30\nDavid 20\n' | sort -k2n
### expect
David 20
Bob 25
Alice 30
### end

### sort_stable
# Stable sort preserves input order for equal keys
printf 'b 1\na 2\nb 3\n' | sort -s -k1,1
### expect
a 2
b 1
b 3
### end

### sort_check
# Check if input is sorted
printf 'a\nb\nc\n' | sort -c
echo $?
### expect
0
### end

### sort_merge
printf 'a\nc\n' > /tmp/f1 && printf 'b\nd\n' > /tmp/f2 && sort -m /tmp/f1 /tmp/f2
### expect
a
b
c
d
### end

### uniq_duplicate_only
printf 'a\na\nb\nc\nc\n' | uniq -d
### expect
a
c
### end

### uniq_unique_only
printf 'a\na\nb\nc\nc\n' | uniq -u
### expect
b
### end

### uniq_ignore_case
# Case insensitive dedup
printf 'a\nA\nb\nB\n' | uniq -i
### expect
a
b
### end

### uniq_skip_fields
# Skip fields before comparing
printf 'x a\ny a\nx b\n' | uniq -f1
### expect
x a
x b
### end

### sort_uniq_count
# Count sorted duplicates
printf 'a\nb\na\nb\na\n' | sort | uniq -c
### expect
      3 a
      2 b
### end

### sort_human_numeric
# Sort human-readable numeric values
printf '10K\n1K\n100M\n1G\n' | sort -h
### expect
1K
10K
100M
1G
### end

### sort_month
# Sort by month name
printf 'Mar\nJan\nFeb\n' | sort -M
### expect
Jan
Feb
Mar
### end

### sort_output_file
# Sort to output file
printf 'b\na\n' | sort -o /tmp/sorted.txt && cat /tmp/sorted.txt
### expect
a
b
### end

### sort_check_unsorted
# Check unsorted input returns 1
printf 'b\na\n' | sort -c 2>/dev/null
echo $?
### expect
1
### end

### sort_key_field_numeric_reverse
# Sort by key with numeric reverse
printf 'x 30\ny 10\nz 20\n' | sort -k2 -n -r
### expect
x 30
z 20
y 10
### end

### sort_field_delim_csv
# Sort CSV by second column
printf 'z,1\na,3\nm,2\n' | sort -t, -k2n
### expect
z,1
m,2
a,3
### end

### uniq_case_count
# Case insensitive count
printf 'Hello\nhello\nHELLO\nWorld\n' | uniq -ic
### expect
      3 Hello
      1 World
### end

### sort_zero_terminated
printf 'b\0a\0c\0' | sort -z | tr '\0' '\n'
### expect
a
b
c
### end

### sort_numeric_prefix_strings
# sort -n extracts leading numeric prefix from strings
printf '0003-msg.md\n0001-msg.md\n0002-msg.md\n' | sort -n
### expect
0001-msg.md
0002-msg.md
0003-msg.md
### end

### sort_numeric_mixed_prefix_lengths
# sort -n with mixed prefix lengths
printf '10-exec\n20-tools\n5-first\n' | sort -n
### expect
5-first
10-exec
20-tools
### end

### sort_numeric_nonnumeric_as_zero
# sort -n treats non-numeric lines as 0, tiebreak lexically
printf 'zzz\n2-second\naaa\n1-first\n' | sort -n
### expect
aaa
zzz
1-first
2-second
### end

### sort_field_delim_k2
# sort -t/ -k2,2
printf 'assemble/20-tools\nassemble/10-init\nassemble/30-end\n' | sort -t/ -k2,2
### expect
assemble/10-init
assemble/20-tools
assemble/30-end
### end

### sort_field_delim_k1
# sort -t/ -k1,1 with equal keys falls back to full line
printf 'z/20-tools\na/10-init\nm/30-end\n' | sort -t/ -k1,1
### expect
a/10-init
m/30-end
z/20-tools
### end

### sort_numeric_reverse
# sort -n -r
printf '1\n3\n2\n' | sort -n -r
### expect
3
2
1
### end

### sort_numeric_zero_padded
# sort -n with zero-padded numbers
printf '003\n010\n001\n' | sort -n
### expect
001
003
010
### end

### sort_version_basic
# sort -V with version numbers
printf '1.10\n1.2\n1.1\n' | sort -V
### expect
1.1
1.2
1.10
### end

### sort_version_semver
# sort -V with semantic versions
printf 'v2.0.1\nv1.9.0\nv2.0.0\nv1.10.0\n' | sort -V
### expect
v1.9.0
v1.10.0
v2.0.0
v2.0.1
### end

### sort_version_files
# sort -V with filenames containing version numbers
printf 'file-1.10.txt\nfile-1.2.txt\nfile-1.1.txt\n' | sort -V
### expect
file-1.1.txt
file-1.2.txt
file-1.10.txt
### end

### sort_version_reverse
# sort -rV reverse version sort
printf '1.1\n1.10\n1.2\n' | sort -rV
### expect
1.10
1.2
1.1
### end

### sort_version_mixed
# sort -V with mixed content
printf 'a1\na10\na2\na20\n' | sort -V
### expect
a1
a2
a10
a20
### end

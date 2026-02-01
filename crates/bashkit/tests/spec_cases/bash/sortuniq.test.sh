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

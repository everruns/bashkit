### column_passthrough
### bash_diff: column not available in all CI environments
# Column without -t fills entries into columns
printf 'hello\nworld\n' | column
### expect
hello	world
### end

### column_table
### bash_diff: column not available in all CI environments
# Column -t aligns columns
printf 'a b c\nfoo bar baz\n' | column -t
### expect
a    b    c
foo  bar  baz
### end

### column_table_colon_sep
### bash_diff: column not available in all CI environments
# Column -t with colon separator
printf 'name:value\nfoo:bar\n' | column -t -s:
### expect
name  value
foo   bar
### end

### column_empty
### bash_diff: column not available in all CI environments
# Empty input
printf '' | column -t
echo done
### expect
done
### end

### column_single_column
### bash_diff: column not available in all CI environments
# Single column table
printf 'alpha\nbeta\ngamma\n' | column -t
### expect
alpha
beta
gamma
### end

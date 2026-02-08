### column_passthrough
# Column without -t passes through
printf 'hello\nworld\n' | column
### expect
hello
world
### end

### column_table
# Column -t aligns columns
printf 'a b c\nfoo bar baz\n' | column -t
### expect
a    b    c
foo  bar  baz
### end

### column_table_colon_sep
# Column -t with colon separator
printf 'name:value\nfoo:bar\n' | column -t -s:
### expect
name  value
foo   bar
### end

### column_empty
# Empty input
printf '' | column -t
echo done
### expect
done
### end

### column_single_column
# Single column table
printf 'alpha\nbeta\ngamma\n' | column -t
### expect
alpha
beta
gamma
### end

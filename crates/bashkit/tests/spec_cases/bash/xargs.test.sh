### xargs_basic_echo
# xargs with default echo
printf "a\nb\nc\n" | xargs
### expect
a b c
### end

### xargs_custom_command
# xargs with custom command
printf "f1\nf2\nf3\n" | xargs echo
### expect
f1 f2 f3
### end

### xargs_replace_string
### skip: xargs -I produces empty output at interpreter level
# xargs -I for replacement
printf "a\nb\n" | xargs -I{} echo "item: {}"
### expect
item: a
item: b
### end

### xargs_max_args
# xargs -n limits args per invocation
printf "1\n2\n3\n4\n" | xargs -n 2 echo
### expect
1 2
3 4
### end

### xargs_null_delim
# xargs -0 uses null delimiter
printf "a\0b\0c" | xargs -0
### expect
a b c
### end

### xargs_custom_delim
# xargs -d uses custom delimiter
echo -n "a,b,c" | xargs -d ','
### expect
a b c
### end

### xargs_empty_input
### bash_diff: VFS xargs echo -n behavior differs
# xargs with empty input
echo -n "" | xargs
### expect

### end

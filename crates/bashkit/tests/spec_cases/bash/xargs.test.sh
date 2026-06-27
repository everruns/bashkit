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

### xargs_max_procs_accepted
# -P is accepted (no longer an "invalid option" error); a single batched
# invocation is order-deterministic and matches real bash.
printf "a\nb\nc\n" | xargs -P 4 echo
### expect
a b c
### end

### xargs_process_slot_var
### bash_diff: real xargs -P runs children in parallel (non-deterministic slot/order); bashkit runs deterministically in order with round-robin slot assignment
# --process-slot-var exposes a distinct slot index (0..N-1) per invocation,
# so sharding logic like `worker $SLOT` works instead of always reading 0.
printf "0\n1\n2\n3\n" | xargs -P 2 --process-slot-var=SLOT -I{} sh -c 'echo {} slot=$SLOT'
### expect
0 slot=0
1 slot=1
2 slot=0
3 slot=1
### end

### xargs_process_slot_var_single_slot
### bash_diff: real xargs parallel scheduling is non-deterministic; bashkit is deterministic
# Without -P, there is a single slot, so the index is always 0 (matches GNU).
printf "x\ny\n" | xargs --process-slot-var=SLOT -I{} sh -c 'echo {} slot=$SLOT'
### expect
x slot=0
y slot=0
### end

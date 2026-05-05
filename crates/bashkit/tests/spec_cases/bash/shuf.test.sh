### shuf_echo_args
# -e shuffles the positional args themselves; sort the output to make
# the test deterministic across rng seeds.
shuf -e apple banana cherry | sort
### expect
apple
banana
cherry
### end

### shuf_input_range
# -i N-M produces every value in the range; sort to compare set
# membership rather than order.
shuf -i 1-5 | sort -n
### expect
1
2
3
4
5
### end

### shuf_head_count_caps
# -n caps the output to N lines (with -e to keep the input deterministic).
shuf -e a b c d e -n 2 | wc -l
### expect
2
### end

### shuf_repeat_requires_n
# -r requires -n in bashkit's safe mode (GNU loops forever otherwise).
shuf -r -e a b c
echo "exit=$?"
### expect
exit=1
### end

### shuf_repeat_with_n_count
# -r samples with replacement; we just check the count, not the values.
shuf -r -e a b c -n 7 | wc -l
### expect
7
### end

### shuf_zero_terminated
# -z uses NUL as separator. Use od -c to render bytes, then grep for
# the lone NUL count to keep the test stable.
printf 'a\nb\n' | shuf -z | od -c | grep -c '\\0'
### expect
1
### end

### shuf_unknown_flag_rejected
# Clap exits 2 on unknown flag; GNU shuf exits 1. The PR's #1532
# scope notes the clap-vs-GNU exit-code divergence; assert non-zero
# here to keep the row stable.
### bash_diff
shuf --bogus
echo "exit=$?"
### expect
exit=2
### end

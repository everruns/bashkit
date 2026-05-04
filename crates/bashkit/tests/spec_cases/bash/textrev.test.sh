### tac_basic
# tac reverses line order
printf "a\nb\nc\n" | tac
### expect
c
b
a
### end

### tac_single_line
# tac with single line
echo "hello" | tac
### expect
hello
### end

### tac_from_file
# tac reads from file
### bash_diff
printf "one\ntwo\nthree\n" > /tmp/tac_test
tac /tmp/tac_test
### expect
three
two
one
### end

### tac_numbered
# tac with numbered lines
printf "1\n2\n3\n4\n5\n" | tac
### expect
5
4
3
2
1
### end

### tac_empty_stdin
# tac with empty input produces no output
echo -n "" | tac
echo "done"
### expect
done
### end

### tac_unknown_flag_rejected
# tac --no-such-flag is rejected by the generated clap parser
tac --no-such-flag 2>/dev/null; echo "exit=$?"
### expect
exit=2
### end

### tac_separator_unimplemented
# bashkit accepts -s at the parser level but errors explicitly until the body lands
tac -s X /tmp/tac_test 2>/dev/null; echo "exit=$?"
### expect
exit=2
### end

### rev_basic
# rev reverses characters on each line
echo "hello" | rev
### expect
olleh
### end

### rev_multiple_lines
# rev reverses each line independently
printf "abc\ndef\nghi\n" | rev
### expect
cba
fed
ihg
### end

### rev_palindrome
# rev on palindrome outputs same word
echo "racecar" | rev
### expect
racecar
### end

### rev_from_file
# rev reads from file
### bash_diff
echo "hello world" > /tmp/rev_test
rev /tmp/rev_test
### expect
dlrow olleh
### end

### rev_empty_stdin
# rev with empty input produces no output
echo -n "" | rev
echo "done"
### expect
done
### end

### rev_spaces
# rev preserves and reverses spaces
echo "a b c" | rev
### expect
c b a
### end

### yes_default
# yes outputs "y" by default (piped through head)
yes | head -3
### expect
y
y
y
### end

### yes_custom_string
# yes with custom string
yes hello | head -2
### expect
hello
hello
### end

### yes_multiple_args
# yes joins multiple args with space
yes a b c | head -2
### expect
a b c
a b c
### end

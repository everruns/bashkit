### append_both_operator
echo a &>> /tmp/ab.txt
{ echo b; echo c >&2; } &>>/tmp/ab.txt
cat /tmp/ab.txt
### expect
a
b
c
### end

### function_dup_before_file_redirect
f() { echo out; echo err >&2; }
f 2>&1 > /tmp/fo.txt | sed 's/^/pipe:/'
f > /tmp/f1.txt 2> /tmp/f2.txt
echo "$(cat /tmp/fo.txt) $(cat /tmp/f1.txt) $(cat /tmp/f2.txt)"
### expect
pipe:err
out out err
### end

### two_heredocs_last_wins
cat <<A <<B
a1
A
b1
B
echo next
### expect
b1
next
### end

### heredoc_on_numbered_fd
paste - - <<A 3<<B
a1
a2
A
b1
B
echo after
### expect
a1	a2
after
### end

### compound_heredoc_then_more_redirects
while read -r l; do echo "[$l]"; done <<A 3<<B >/tmp/w.txt
w1
A
ignored
B
cat /tmp/w.txt
### expect
[w1]
### end

### heredoc_then_words
printf 'x\n' > /tmp/hw.txt
cat <<A /tmp/hw.txt
ignored
A
### expect
x
### end

### paste_dashes_share_stdin
printf '1\n2\n3\n' | paste - -
printf '1\n2\n' | paste -d, - - -
### expect
1	2
3	
1,2,
### end

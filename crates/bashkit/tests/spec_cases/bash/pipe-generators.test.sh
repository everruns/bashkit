# Generators at the head of a pipeline stop when the reader goes away
# (SIGPIPE, status 141) instead of running to an output cap.

### yes_head_sigpipe
yes | head -2
echo "${PIPESTATUS[*]}"
### expect
y
y
141 0
### end

### seq_million_head
seq 1000000 | head -1
echo "${PIPESTATUS[*]}"
### expect
1
141 0
### end

### seq_full_output_through_pipe
seq 5000 | tail -1
seq -s, 5 | cat
seq 3 | wc -l
### expect
5000
1,2,3,4,5
3
### end

### yes_custom_text_head_bytes
yes ab | head -c 7
echo
### expect
ab
ab
a
### end

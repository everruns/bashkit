### bash_builtin_stdin_pipe_to_script
# echo | bash script.sh should forward stdin
echo 'echo "got: $(cat)"' > /tmp/stdin_test.sh
echo "hello" | bash /tmp/stdin_test.sh
### expect
got: hello
### end

### bash_builtin_stdin_pipe_to_c
# echo | bash -c 'cat' should forward stdin
echo "piped" | bash -c 'cat'
### expect
piped
### end

### bash_builtin_stdin_read
# echo | bash script.sh with read builtin
echo 'read -r line; echo "line: $line"' > /tmp/read_test.sh
echo "test input" | bash /tmp/read_test.sh
### expect
line: test input
### end

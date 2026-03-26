### awk_dev_stderr_redirect
# print > "/dev/stderr" goes to stderr, not VFS
result=$(echo "test" | awk '{print "error msg" > "/dev/stderr"; print "stdout msg"}' 2>/dev/null)
echo "$result"
### expect
stdout msg
### end

### awk_dev_stdout_redirect
# print > "/dev/stdout" goes to stdout
echo "hello" | awk '{print "via stdout" > "/dev/stdout"}'
### expect
via stdout
### end

### awk_dev_stderr_append
# print >> "/dev/stderr" also goes to stderr
result=$(echo "test" | awk '{print "err1" >> "/dev/stderr"; print "out1"}' 2>/dev/null)
echo "$result"
### expect
out1
### end

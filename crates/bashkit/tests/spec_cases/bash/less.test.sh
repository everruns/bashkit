### less_basic
### bash_diff: less outputs full file without paging in VFS
# less displays file content
echo "line one" > /tmp/less_test.txt
echo "line two" >> /tmp/less_test.txt
less /tmp/less_test.txt
### expect
line one
line two
### end

### less_stdin
### bash_diff: less outputs full input without paging in VFS
# less reads from stdin
echo "from stdin" | less
### expect
from stdin
### end

### less_nonexistent
### bash_diff: VFS less error behavior differs from real less
### exit_code: 1
# less on nonexistent file
less /tmp/nonexistent_less_file
### expect
### end

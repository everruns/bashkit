# tar create tests
# Tests tar -c with VFS files and -C flag (issue #1118)

### tar_create_basic
# tar -c creates an archive from VFS files
echo "content" > /tmp/test.txt
tar -c -f /tmp/test.tar /tmp/test.txt
test -f /tmp/test.tar && echo "archive created"
### expect
archive created
### end

### tar_create_with_C_flag
# tar -c -C resolves files relative to the given directory
echo "hello" > /tmp/file.txt
tar -c -f /tmp/out.tar -C /tmp file.txt
test -f /tmp/out.tar && echo "archive created"
### expect
archive created
### end

### tar_create_gzip
# tar -czf creates a gzip archive
echo "data" > /tmp/gz.txt
tar -c -z -f /tmp/gz.tar.gz -C /tmp gz.txt
test -f /tmp/gz.tar.gz && echo "archive created"
### expect
archive created
### end

### tar_create_and_list
# Create a tar archive and list its contents
mkdir -p /tmp/tartest
echo "hello" > /tmp/tartest/file1.txt
echo "world" > /tmp/tartest/file2.txt
tar -cf /tmp/test.tar /tmp/tartest/file1.txt /tmp/tartest/file2.txt
tar -tf /tmp/test.tar | sort
### expect
/tmp/tartest/file1.txt
/tmp/tartest/file2.txt
### end

### tar_create_and_extract_stdout
# Create then extract to stdout with -O
mkdir -p /tmp/tsrc
echo "data" > /tmp/tsrc/a.txt
tar -cf /tmp/tout.tar /tmp/tsrc/a.txt
tar -xf /tmp/tout.tar -O
### expect
data
### end

### tar_verbose_create
# Verbose output when creating
mkdir -p /tmp/vtest
echo "x" > /tmp/vtest/f.txt
tar -cvf /tmp/v.tar /tmp/vtest/f.txt 2>&1
### expect
/tmp/vtest/f.txt
### end

### tar_gzip_roundtrip
# Create and extract gzip archive, verify via -O
mkdir -p /tmp/gz
echo "compressed" > /tmp/gz/c.txt
tar -czf /tmp/gz.tar.gz /tmp/gz/c.txt
tar -xzf /tmp/gz.tar.gz -O
### expect
compressed
### end

### tar_no_args
### exit_code: 2
# tar with no arguments
tar
### expect
### end

### tar_directory_recursive
# tar handles directories recursively
mkdir -p /tmp/tdeep/sub
echo "a" > /tmp/tdeep/top.txt
echo "b" > /tmp/tdeep/sub/bot.txt
tar -cf /tmp/tdeep.tar /tmp/tdeep
tar -tf /tmp/tdeep.tar | sort
### expect
/tmp/tdeep/
/tmp/tdeep/sub/
/tmp/tdeep/sub/bot.txt
/tmp/tdeep/top.txt
### end

### tar_missing_file
### exit_code: 2
# tar on nonexistent file
tar -cf /tmp/bad.tar /nonexistent/path
### expect
### end

### tar_create_empty
### exit_code: 2
# tar refuses to create empty archive
tar -cf /tmp/empty.tar
### expect
### end

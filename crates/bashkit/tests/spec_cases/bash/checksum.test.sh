### md5sum_stdin
# md5sum from stdin
echo -n "hello" | md5sum
### expect
5d41402abc4b2a76b9719d911017c592  -
### end

### sha1sum_stdin
# sha1sum from stdin
echo -n "hello" | sha1sum
### expect
aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d  -
### end

### sha256sum_stdin
# sha256sum from stdin
echo -n "hello" | sha256sum
### expect
2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  -
### end

### md5sum_empty
# md5sum of empty string
echo -n "" | md5sum
### expect
d41d8cd98f00b204e9800998ecf8427e  -
### end

### sha256sum_newline
# sha256sum with trailing newline
echo "hello" | sha256sum
### expect
5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03  -
### end

### md5sum_file
# md5sum of a file
echo -n "test" > /tmp/checkfile.txt
md5sum /tmp/checkfile.txt
### expect
098f6bcd4621d373cade4e832627b4f6  /tmp/checkfile.txt
### end

### sha1sum_file
# sha1sum of a file
echo -n "test" > /tmp/checkfile.txt
sha1sum /tmp/checkfile.txt
### expect
a94a8fe5ccb19ba61c4c0873d391e987982fbbd3  /tmp/checkfile.txt
### end

### sha256sum_file
# sha256sum of a file
echo -n "test" > /tmp/checkfile.txt
sha256sum /tmp/checkfile.txt
### expect
9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08  /tmp/checkfile.txt
### end

### md5sum_multiple_files
# md5sum of multiple files
echo -n "aaa" > /tmp/a.txt
echo -n "bbb" > /tmp/b.txt
md5sum /tmp/a.txt /tmp/b.txt
### expect
47bce5c74f589f4867dbd57e9ca9f808  /tmp/a.txt
08f8e0260c64418510cefb2b06eee5cd  /tmp/b.txt
### end

### checksum_missing_file
# checksum of non-existent file
md5sum /tmp/nonexistent.txt
### expect_exit_code
1
### end

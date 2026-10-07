### tar_create_and_list
### bash_diff: VFS tar stores absolute paths differently than real tar
# Create a tar archive and list its contents
mkdir -p /tmp/tartest
echo "hello" > /tmp/tartest/file1.txt
echo "world" > /tmp/tartest/file2.txt
tar -cf /tmp/test.tar /tmp/tartest/file1.txt /tmp/tartest/file2.txt
tar -tf /tmp/test.tar | sort
### expect
tmp/tartest/file1.txt
tmp/tartest/file2.txt
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
### bash_diff: real bash sees the remapped sandbox path
# Verbose output when creating goes to stdout, with the name as given
mkdir -p /tmp/vtest
echo "x" > /tmp/vtest/f.txt
tar -cvf /tmp/v.tar /tmp/vtest/f.txt 2>/dev/null
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
### bash_diff: VFS tar stores absolute paths differently
# tar handles directories recursively
mkdir -p /tmp/tdeep/sub
echo "a" > /tmp/tdeep/top.txt
echo "b" > /tmp/tdeep/sub/bot.txt
tar -cf /tmp/tdeep.tar /tmp/tdeep
tar -tf /tmp/tdeep.tar | sort
### expect
tmp/tdeep/
tmp/tdeep/sub/
tmp/tdeep/sub/bot.txt
tmp/tdeep/top.txt
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

### tar_strips_leading_slash
mkdir -p /tmp/tsl/d && echo hi > /tmp/tsl/d/f
tar -cf /tmp/tsl.tar /tmp/tsl/d 2>/tmp/tsl.err; echo "rc=$?"
cat /tmp/tsl.err
tar -tf /tmp/tsl.tar | grep -c '^/'
tar -tf /tmp/tsl.tar | grep -c 'tsl/d/f$'
### expect
rc=0
tar: Removing leading `/' from member names
0
1
### end

### tar_verbose_create_goes_to_stdout
cd /tmp/ && mkdir -p tvs && echo hi > tvs/f
tar -cvf /tmp/tvs.tar tvs 2>/dev/null
### expect
tvs/
tvs/f
### tar_strip_components
# --strip-components drops leading path components on extract
mkdir -p /tmp/sc/proj/src; echo a > /tmp/sc/proj/src/a.txt; echo r > /tmp/sc/proj/README
tar -czf /tmp/sc.tgz -C /tmp/sc proj
mkdir -p /tmp/sc_out; tar -xzf /tmp/sc.tgz -C /tmp/sc_out --strip-components=1
cat /tmp/sc_out/README /tmp/sc_out/src/a.txt
mkdir -p /tmp/sc_out2; tar --extract --gzip --file /tmp/sc.tgz --directory /tmp/sc_out2 --strip-components 2
cat /tmp/sc_out2/a.txt
ls /tmp/sc_out2
### expect
r
a
a
a.txt
### end

### tar_extract_named_members
# Only the named members (and everything under a named directory) are extracted
mkdir -p /tmp/mem/p/d; echo 1 > /tmp/mem/p/one; echo 2 > /tmp/mem/p/d/two; echo 3 > /tmp/mem/p/three
tar -cf /tmp/mem.tar -C /tmp/mem p
mkdir -p /tmp/mem_out; tar -xf /tmp/mem.tar -C /tmp/mem_out p/one p/d
find /tmp/mem_out -type f | sort
tar -xf /tmp/mem.tar -C /tmp/mem_out p/missing; echo "rc=$?"
### expect
/tmp/mem_out/p/d/two
/tmp/mem_out/p/one
rc=2
### end

### tar_exclude
# --exclude skips matching names when creating
mkdir -p /tmp/ex/p; echo a > /tmp/ex/p/a.txt; echo l > /tmp/ex/p/b.log; mkdir -p /tmp/ex/p/node_modules; echo x > /tmp/ex/p/node_modules/x.js
tar -cf /tmp/ex.tar --exclude='*.log' --exclude=node_modules -C /tmp/ex p
tar -tf /tmp/ex.tar | grep -v '/$' | sort
### expect
p/a.txt
### end

### tar_long_mode_options
mkdir -p /tmp/lm; echo z > /tmp/lm/z
cd /tmp/ && tar --create --file=/tmp/lm.tar lm
tar --list --file /tmp/lm.tar | grep -v '/$'
tar --unknown-thing -tf /tmp/lm.tar 2>/dev/null || echo rejected
### expect
lm/z
rejected
### end

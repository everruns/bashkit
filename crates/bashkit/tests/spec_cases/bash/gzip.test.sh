### gzip_via_tar
# gzip compression works via tar -z, extract to stdout
mkdir -p /tmp/gztest
echo "gzip content" > /tmp/gztest/g.txt
tar -czf /tmp/gztest.tar.gz /tmp/gztest/g.txt
tar -xzf /tmp/gztest.tar.gz -O
### expect
gzip content
### end

### gzip_large_file
### bash_diff: VFS tar stores absolute paths differently
# gzip handles larger content
mkdir -p /tmp/gzlarge
seq 1 100 > /tmp/gzlarge/nums.txt
tar -czf /tmp/large.tar.gz /tmp/gzlarge/nums.txt
tar -tzf /tmp/large.tar.gz
### expect
/tmp/gzlarge/nums.txt
### end

### gzip_stdout_roundtrip
# gzip -c / gunzip -c stream through stdout and keep the input file
printf 'hello gz\n' > /tmp/gzc.txt
gzip -c /tmp/gzc.txt > /tmp/gzc.txt.gz
test -f /tmp/gzc.txt && echo kept
gunzip -c /tmp/gzc.txt.gz
printf 'pipe data\n' | gzip -9 | gzip -dc
### expect
kept
hello gz
pipe data
### end

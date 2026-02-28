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
# gzip handles larger content
mkdir -p /tmp/gzlarge
seq 1 100 > /tmp/gzlarge/nums.txt
tar -czf /tmp/large.tar.gz /tmp/gzlarge/nums.txt
tar -tzf /tmp/large.tar.gz
### expect
/tmp/gzlarge/nums.txt
### end

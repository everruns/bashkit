### tee_basic
# tee writes to file and stdout
echo "hello" | tee /tmp/tee_out.txt
### expect
hello
### end

### tee_file_contents
# tee creates file with correct contents
echo "tee data" | tee /tmp/tee_check.txt > /dev/null
cat /tmp/tee_check.txt
### expect
tee data
### end

### tee_append
# tee -a appends to file
echo "first" > /tmp/tee_append.txt
echo "second" | tee -a /tmp/tee_append.txt > /dev/null
cat /tmp/tee_append.txt
### expect
first
second
### end

### tee_multiple_files
# tee writes to multiple files
echo "multi" | tee /tmp/tee_m1.txt /tmp/tee_m2.txt > /dev/null
cat /tmp/tee_m1.txt
cat /tmp/tee_m2.txt
### expect
multi
multi
### end

### tee_overwrite
# tee overwrites existing file by default
echo "old" > /tmp/tee_ow.txt
echo "new" | tee /tmp/tee_ow.txt > /dev/null
cat /tmp/tee_ow.txt
### expect
new
### end

### tee_multiline
# tee handles multiline input
printf "line1\nline2\nline3\n" | tee /tmp/tee_ml.txt > /dev/null
cat /tmp/tee_ml.txt
### expect
line1
line2
line3
### end

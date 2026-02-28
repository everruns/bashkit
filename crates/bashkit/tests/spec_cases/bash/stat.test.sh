### stat_basic
# stat shows file information
echo "hello" > /tmp/stat_test.txt
stat /tmp/stat_test.txt | head -1
### expect
  File: /tmp/stat_test.txt
### end

### stat_format_name
# stat -c %n prints file name
echo "x" > /tmp/stat_name.txt
stat -c '%n' /tmp/stat_name.txt
### expect
/tmp/stat_name.txt
### end

### stat_format_size
# stat -c %s prints size
printf "abcde" > /tmp/stat_size.txt
stat -c '%s' /tmp/stat_size.txt
### expect
5
### end

### stat_format_type
# stat -c %F prints file type
mkdir -p /tmp/stat_dir
stat -c '%F' /tmp/stat_dir
### expect
directory
### end

### stat_format_type_regular
# stat -c %F on regular file
echo "x" > /tmp/stat_reg.txt
stat -c '%F' /tmp/stat_reg.txt
### expect
regular file
### end

### stat_nonexistent
### exit_code: 1
# stat on nonexistent file
stat /tmp/nonexistent_stat_xyz
### expect
### end

### stat_format_combined
# stat with combined format string
echo "hi" > /tmp/stat_combo.txt
stat -c '%n %F' /tmp/stat_combo.txt
### expect
/tmp/stat_combo.txt regular file
### end

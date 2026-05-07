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

### stat_unknown_flag_rejected
### bash_diff: clap-backed stat returns exit 2 for parse errors; GNU stat returns 1
# Unknown long flag rejected with usage error
stat --no-such-flag /tmp/stat_combo.txt 2>/dev/null; echo "exit=$?"
### expect
exit=2
### end

### stat_missing_format_value
### bash_diff: clap-backed stat returns exit 2 when -c lacks a value; GNU stat returns 1
# -c without a value is a parse error
stat -c 2>/dev/null; echo "exit=$?"
### expect
exit=2
### end

### stat_filesystem_unsupported
### bash_diff: bashkit rejects --file-system (no VFS block-stat surface); GNU stat reports host filesystem stats
# --file-system isn't VFS-shaped; we surface that rather than silently no-op
stat -f /tmp/stat_combo.txt 2>/dev/null; echo "exit=$?"
### expect
exit=1
### end

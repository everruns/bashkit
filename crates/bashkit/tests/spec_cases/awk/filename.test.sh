### awk_filename_single_file
# FILENAME is set when processing a single file
echo -e "line1\nline2" > /tmp/f1.txt
awk '{print FILENAME, $0}' /tmp/f1.txt
### expect
/tmp/f1.txt line1
/tmp/f1.txt line2
### end

### awk_filename_multi_file
# FILENAME changes between files
echo "alpha" > /tmp/a.txt
echo "beta" > /tmp/b.txt
awk '{print FILENAME, $0}' /tmp/a.txt /tmp/b.txt
### expect
/tmp/a.txt alpha
/tmp/b.txt beta
### end

### awk_filename_fnr_reset
# FNR resets per file, FILENAME tracks current file
echo -e "x\ny" > /tmp/p.txt
echo -e "a\nb" > /tmp/q.txt
awk '{print FILENAME, FNR, $0}' /tmp/p.txt /tmp/q.txt
### expect
/tmp/p.txt 1 x
/tmp/p.txt 2 y
/tmp/q.txt 1 a
/tmp/q.txt 2 b
### end

### awk_filename_stdin_empty
# FILENAME is empty when reading from stdin
echo "hello" | awk '{print "file=[" FILENAME "]", $0}'
### expect
file=[] hello
### end

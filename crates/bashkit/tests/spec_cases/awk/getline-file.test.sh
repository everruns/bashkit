### awk_getline_file_into_var
# getline var < "file" reads lines one at a time into variable
echo -e "alpha\nbeta\ngamma" > /tmp/data.txt
awk 'BEGIN{while((getline line < "/tmp/data.txt") > 0) print line}'
### expect
alpha
beta
gamma
### end

### awk_getline_file_no_var
# getline < "file" without variable updates $0
echo -e "first\nsecond" > /tmp/data.txt
awk 'BEGIN{getline < "/tmp/data.txt"; print}'
### expect
first
### end

### awk_getline_file_multiple_reads
# Multiple getline calls advance through the file
echo -e "line1\nline2\nline3" > /tmp/data.txt
awk 'BEGIN{getline a < "/tmp/data.txt"; getline b < "/tmp/data.txt"; print a; print b}'
### expect
line1
line2
### end

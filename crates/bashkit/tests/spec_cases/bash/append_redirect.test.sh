# Append redirect tests
# Tests that >> does not duplicate content in compound commands (issue #1119)

### append_simple
# Simple append works
echo "line1" > /tmp/append.txt
echo "line2" >> /tmp/append.txt
cat /tmp/append.txt
### expect
line1
line2
### end

### append_brace_group
# Brace group append writes once
rm -f /tmp/brace.txt
{ echo "line1"; echo "line2"; } >> /tmp/brace.txt
cat /tmp/brace.txt
### expect
line1
line2
### end

### append_brace_group_existing_file
# Brace group append to existing file
echo "existing" > /tmp/exist.txt
{ echo "new1"; echo "new2"; } >> /tmp/exist.txt
cat /tmp/exist.txt
### expect
existing
new1
new2
### end

### append_no_duplicate
# Repeated appends should not duplicate
rm -f /tmp/nodup.txt
echo "first" >> /tmp/nodup.txt
echo "second" >> /tmp/nodup.txt
cat /tmp/nodup.txt
### expect
first
second
### end

### append_if_else
# if/else with append redirect
rm -f /tmp/ifelse.txt
if false; then
    echo "yes"
else
    echo "no"
fi >> /tmp/ifelse.txt
cat /tmp/ifelse.txt
### expect
no
### end

### append_rebuild_cycle
# Simulate bashblog rebuild: create file, then append-only should not duplicate
rm -f /tmp/footer.html
{
    echo "<div>footer line 1</div>"
    echo "<div>footer line 2</div>"
} >> /tmp/footer.html
# Rebuild: run the same block again — should append, not duplicate first block
{
    echo "<div>footer line 1</div>"
    echo "<div>footer line 2</div>"
} >> /tmp/footer.html
wc -l < /tmp/footer.html
### expect
4
### end

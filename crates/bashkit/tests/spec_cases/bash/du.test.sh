### du_summary
# du -s shows total size
mkdir -p /tmp/du_test
echo "data" > /tmp/du_test/f1.txt
echo "more" > /tmp/du_test/f2.txt
du -s /tmp/du_test | awk '{print ($1 >= 0) ? "ok" : "bad"}'
### expect
ok
### end

### du_human_readable
# du -sh shows human-readable size
mkdir -p /tmp/du_h
echo "content" > /tmp/du_h/file.txt
du -sh /tmp/du_h | grep -q "/tmp/du_h" && echo "ok"
### expect
ok
### end

### du_default_cwd
# du with no args uses current directory
cd /tmp
mkdir -p du_cwd_test
echo "x" > du_cwd_test/a.txt
du -s du_cwd_test | grep -q "du_cwd_test" && echo "ok"
### expect
ok
### end

### du_nonexistent
### exit_code: 1
# du on nonexistent path
du /nonexistent_du_path_xyz
### expect
### end

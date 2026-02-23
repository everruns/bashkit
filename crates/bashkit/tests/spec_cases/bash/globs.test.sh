### glob_star
### bash_diff: Bashkit VFS has files, real bash CI filesystem does not - glob expands differently
# Glob with asterisk
echo a > /test1.txt; echo b > /test2.txt; echo /test*.txt
### expect
/test1.txt /test2.txt
### end

### glob_question
### bash_diff: Bashkit VFS has files, real bash CI filesystem does not - glob expands differently
# Glob with question mark
echo a > /a1.txt; echo b > /a2.txt; echo c > /a10.txt; echo /a?.txt
### expect
/a1.txt /a2.txt
### end

### glob_no_match
# Glob with no matches returns pattern
echo /nonexistent/*.xyz
### expect
/nonexistent/*.xyz
### end

### glob_in_quotes
# Glob in quotes not expanded
echo "/*.txt"
### expect
/*.txt
### end

### glob_bracket
### bash_diff: Bashkit VFS has files, real bash CI filesystem does not - glob expands differently
echo a > /x1.txt; echo b > /x2.txt; echo /x[12].txt
### expect
/x1.txt /x2.txt
### end

### glob_recursive
### bash_diff: Bashkit VFS has files, real bash CI filesystem does not
# Recursive glob with **
mkdir -p /recur/sub1/deep
mkdir -p /recur/sub2
echo a > /recur/f1.txt
echo b > /recur/sub1/f2.txt
echo c > /recur/sub1/deep/f3.txt
echo d > /recur/sub2/f4.txt
echo /recur/**/*.txt
### expect
/recur/f1.txt /recur/sub1/deep/f3.txt /recur/sub1/f2.txt /recur/sub2/f4.txt
### end

### glob_brace
echo file.{txt,log}
### expect
file.txt file.log
### end

### glob_in_for_loop
### bash_diff: Bashkit VFS has files, real bash CI filesystem does not - glob expands differently
# Glob expansion in for-loop word list
echo a > /g1.txt; echo b > /g2.txt
for f in /g*.txt; do echo $f; done
### expect
/g1.txt
/g2.txt
### end

### glob_in_for_no_match
# Glob with no matches in for-loop keeps literal pattern
for f in /nonexistent_dir/*.xyz; do echo $f; done
### expect
/nonexistent_dir/*.xyz
### end

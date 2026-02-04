### glob_star
### bash_diff: BashKit VFS has files, real bash CI filesystem does not - glob expands differently
# Glob with asterisk
echo a > /test1.txt; echo b > /test2.txt; echo /test*.txt
### expect
/test1.txt /test2.txt
### end

### glob_question
### bash_diff: BashKit VFS has files, real bash CI filesystem does not - glob expands differently
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
### bash_diff: BashKit VFS has files, real bash CI filesystem does not - glob expands differently
echo a > /x1.txt; echo b > /x2.txt; echo /x[12].txt
### expect
/x1.txt /x2.txt
### end

### glob_recursive
### skip: recursive glob (**) not implemented
echo /**/*.txt
### expect
### end

### glob_brace
echo file.{txt,log}
### expect
file.txt file.log
### end

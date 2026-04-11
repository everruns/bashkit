### cat_help
# cat --help should show usage
cat --help | head -1
### expect
Usage: cat [OPTION]... [FILE]...
### end

### grep_help
# grep --help should show usage
grep --help | head -1
### expect
Usage: grep [OPTION]... PATTERN [FILE]...
### end

### sort_help
# sort --help should show usage
sort --help | head -1
### expect
Usage: sort [OPTION]... [FILE]...
### end

### ls_help
# ls --help should show usage
ls --help | head -1
### expect
Usage: ls [OPTION]... [FILE]...
### end

### date_help
# date --help should show usage
date --help | head -1
### expect
Usage: date [+FORMAT] [-u] [-R] [-I[TIMESPEC]] [-d STRING] [-r FILE]
### end

### cat_version
# cat --version should output version info
cat --version
### expect
cat (bashkit) 0.1
### end

### grep_version
# grep --version should output version info
grep --version
### expect
grep (bashkit) 0.1
### end

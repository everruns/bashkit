### cat_help
### bash_diff: bashkit custom help text differs from real coreutils
# cat --help should show usage
cat --help | head -1
### expect
Usage: cat [OPTION]... [FILE]...
### end

### grep_help
### bash_diff: bashkit custom help text differs from real grep
# grep --help should show usage
grep --help | head -1
### expect
Usage: grep [OPTION]... PATTERN [FILE]...
### end

### sort_help
### bash_diff: bashkit custom help text differs from real coreutils
# sort --help should show usage
sort --help | head -1
### expect
Usage: sort [OPTION]... [FILE]...
### end

### ls_help
### bash_diff: bashkit custom help text differs from real coreutils
# ls --help should show usage
ls --help | head -1
### expect
Usage: ls [OPTION]... [FILE]...
### end

### date_help
### bash_diff: bashkit custom help text differs from real coreutils
# date --help should show usage
date --help | head -1
### expect
Usage: date [+FORMAT] [-u] [-R] [-I[TIMESPEC]] [-d STRING] [-r FILE]
### end

### cat_version
### bash_diff: bashkit reports its own version string
# cat --version should output version info; format is `cat <crate-version>`
cat --version | sed -E 's/^(cat) [0-9]+\.[0-9]+\.[0-9]+.*/\1 X.Y.Z/'
### expect
cat X.Y.Z
### end

### grep_version
### bash_diff: bashkit reports its own version string
# grep --version should output version info
grep --version
### expect
grep (bashkit) 0.1
### end

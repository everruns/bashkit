### basename_simple
# Get filename from path
basename /usr/bin/sort
### expect
sort
### end

### basename_with_suffix
# Remove suffix from filename
basename file.txt .txt
### expect
file
### end

### basename_no_suffix_match
# Suffix doesn't match - keep full name
basename file.txt .doc
### expect
file.txt
### end

### basename_no_dir
# Just a filename
basename filename
### expect
filename
### end

### basename_trailing_slash
# Path with trailing slash
basename /usr/bin/
### expect
bin
### end

### dirname_simple
# Get directory from path
dirname /usr/bin/sort
### expect
/usr/bin
### end

### dirname_no_dir
# Just a filename - return current dir
dirname filename
### expect
.
### end

### dirname_root
# Root directory
dirname /
### expect
/
### end

### dirname_trailing_slash
# Path with trailing slash
dirname /usr/bin/
### expect
/usr
### end

### dirname_relative
# Relative path with subdirectory
dirname foo/bar/baz
### expect
foo/bar
### end

### basename_no_args
### skip: missing argument error handling not implemented
# Basename with no arguments should error
basename 2>/dev/null
echo $?
### expect
1
### end

### dirname_no_args
### skip: missing argument error handling not implemented
# Dirname with no arguments should error
dirname 2>/dev/null
echo $?
### expect
1
### end

### basename_multiple_slashes
# Handle multiple trailing slashes
basename /path///to///file///
### expect
file
### end

### dirname_single_component
# Single component relative path
dirname file
### expect
.
### end

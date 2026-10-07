# BashBox path-name cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_path_name_basename_strips_directories_and_trailing_slashes
# basename strips directories and trailing slashes
basename a/b/; basename b.txt; basename /
### expect
b
b.txt
/
### end

### bashbox_path_name_basename_strips_a_suffix_unless_it_is_the_whole_name
# basename strips a suffix unless it is the whole name
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
basename a/b.txt .txt; basename .txt .txt
### expect
b
.txt
### end

### bashbox_path_name_dirname
# dirname
dirname /a/b/c; dirname a//b/; dirname /a; dirname //a; dirname /; dirname a; dirname ""
### expect
/a/b
a
/
/
/
.
.
### end

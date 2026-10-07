# BashBox find-predicates cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_find_predicates_newer_with_h_follows_the_reference
# -newer with -H follows the reference
find -H t/fl -newer t/fl
### expect
### end

### bashbox_find_predicates_inum_0
# -inum 0
find t -inum 0
### expect
### end

### bashbox_find_predicates_user_root
# -user root
find t -user root
### expect
### end

### bashbox_find_predicates_nouser_and_nogroup
# -nouser and -nogroup
find t -nouser -o -nogroup
### expect
### end

### bashbox_find_predicates_newermt_is_strictly_newer
# -newermt is strictly newer
find t -newermt '2020-01-02 03:04:05' -name f
### expect
### end

### bashbox_find_predicates_fprint_creates_the_file_even_without_matches
# -fprint creates the file even without matches
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo old > out; find t -name nomatch -fprint out; wc -c < out
### expect
0
### end

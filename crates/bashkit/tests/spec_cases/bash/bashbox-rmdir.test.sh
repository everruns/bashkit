# BashBox rmdir cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_rmdir_removes_empty_directories
# removes empty directories
rmdir -v a/b/c; ls a/b
### expect
rmdir: removing directory, 'a/b/c'
### end

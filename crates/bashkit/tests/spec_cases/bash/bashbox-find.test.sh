# BashBox find cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_find_name_is_case_sensitive
# -name is case sensitive
find . -name "main.php"
### expect
### end

### bashbox_find_file_start_point_filtered_out
# file start point filtered out
find a.txt -type d
### expect
### end

### bashbox_find_fractional_mtime
# fractional -mtime
find proj -type f -mtime -1.5 -mtime +0.5
### expect
### end

### bashbox_find_mmin_n_matches_the_minute_before
# -mmin N matches the minute before
find proj -type f -mmin 30 -o -type f -mmin 5
### expect
### end

### bashbox_find_perm_symbolic
# -perm symbolic
find proj ! -type l -perm u=rw,go=r -name 's*'
### expect
### end

### bashbox_find_perm_copying_another_class
# -perm copying another class
find proj ! -type l -perm u=rw,g=u,o=u
### expect
### end

### bashbox_find_perm_with_nothing
# -perm = with nothing
find proj ! -type l -perm =
### expect
### end

### bashbox_find_perm_t_and_t
# -perm =t and +t
find proj -perm +t
### expect
### end

### bashbox_find_quit_alone_prints_nothing
# -quit alone prints nothing
find proj -quit
### expect
### end

### bashbox_find_execdir_from_the_root
# -execdir from the root
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
find / -maxdepth 0 -execdir echo {} \;
### expect
/
### end

### bashbox_find_printf_h_and_f_of_the_root
# -printf %h and %f of the root
find / -maxdepth 0 -printf '[%h][%f]\n'
### expect
[][/]
### end

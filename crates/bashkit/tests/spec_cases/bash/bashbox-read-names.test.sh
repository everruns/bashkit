# BashBox read-names cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_read_names_an_option_after_a_name_is_an_invalid_identifier
# an option after a name is an invalid identifier
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
read a -r <<< 'x y'; echo "$? [$a]"
### expect
1 [x]
### end

### bashbox_read_names_names_before_a_bad_one_are_assigned
# names before a bad one are assigned
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
read a 1x b <<< 'x y z'; echo "$? [$a] [$b]"
### expect
1 [x] []
### end

### bashbox_read_names_a_bad_name_is_reported_at_end_of_input_too
# a bad name is reported at end of input too
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
read a -r < /dev/null; echo $?
### expect
1
### end

### bashbox_read_names_an_array_element_is_a_valid_name
# an array element is a valid name
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
read 'a[1]' b <<< 'x y'; echo "$? [${a[1]}] [$b]"
### expect
0 [x] [y]
### end

### bashbox_read_names_read_a_needs_a_plain_name
# read -a needs a plain name
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
read -a 1x <<< 'x'; echo $?; read -a 'a[1]' <<< 'x'; echo $?
### expect
1
1
### end

### bashbox_read_names_read_n_with_a_bad_name
# read -N with a bad name
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
read -N 2 a 1x <<< 'xyz'; echo "$? [$a]"
### expect
1 [xy]
### end

### bashbox_read_names_read_n_into_an_array
# read -N into an array
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
read -N 3 -a arr <<< 'x y'; echo "$? [${arr[0]}] ${#arr[@]}"
### expect
0 [x y] 1
### end

### bashbox_read_names_read_n_a_with_a_bad_name
# read -N -a with a bad name
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
read -N 1 -a 1x <<< 'x'; echo $?
### expect
1
### end

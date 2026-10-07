# BashBox cut cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_cut_fields_with_a_delimiter
# fields with a delimiter
printf 'a:b:c:d\nnodelim\n' | cut -d: -f1,3-
### expect
a:c:d
nodelim
### end

### bashbox_cut_fields_default_to_tab_delimited
# fields default to tab-delimited
printf 'a\tb\n' | cut -f2
### expect
b
### end

### bashbox_cut_separate_delimiter_argument_and_open_start_range
# separate delimiter argument and open start range
echo a:b:c | cut -d : -f -2
### expect
a:b
### end

### bashbox_cut_empty_input
# empty input
printf '' | cut -f1
### expect
### end

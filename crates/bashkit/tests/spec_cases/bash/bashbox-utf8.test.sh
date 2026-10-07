# BashBox utf8 cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_utf8_length_counts_characters
# length counts characters
x='héllo wörld'; echo ${#x}; y='日本語'; echo ${#y}
### expect
11
3
### end

### bashbox_utf8_array_element_length
# array element length
a=('héllo' 'ö'); echo "${#a[0]} ${#a[1]} ${#a[@]}"
### expect
5 1 2
### end

### bashbox_utf8_positional_length
# positional length
set -- 'é' 'ab'; echo ${#1} ${#2}
### expect
1 2
### end

### bashbox_utf8_substring_by_character
# substring by character
x='héllo'; echo "${x:1:2}|${x: -3}|${x:0:1}|${x:4}|${x:(-4):2}"
### expect
él|llo|h|o|él
### end

### bashbox_utf8_substring_of_wide_characters
# substring of wide characters
x='日本語'; echo "${x:1} ${x: -1:1}"
### expect
本語 語
### end

### bashbox_utf8_invalid_utf_8_counts_bytes
# invalid UTF-8 counts bytes
x=$'\xff\xfe'; echo ${#x}; y=$'a\xffb'; echo ${#y} "${y:2}"
### expect
2
3 b
### end

### bashbox_utf8_patterns_match_characters
# patterns match characters
x='héllo'; echo "${x#?}|${x%??}|${x/?/X}|${x//[é]/E}|${x/#h?/Y}"
### expect
éllo|hél|Xéllo|hEllo|Yllo
### end

### bashbox_utf8_case_and_patterns
# case and [[ ]] patterns
x='héllo'; case $x in h?llo) echo case;; esac; [[ $x == h?llo ]] && echo glob; [[ $x =~ ^h.llo$ ]] && echo regex
### expect
case
glob
regex
### end

### bashbox_utf8_filename_globbing_by_character
# filename globbing by character
touch é1 ab1; echo ?1
### expect
é1
### end

### bashbox_utf8_patterns_on_invalid_utf_8
# patterns on invalid UTF-8
x=$'\xffa'; echo "${x#?}"; case $x in ?a) echo case;; esac
### expect
a
case
### end

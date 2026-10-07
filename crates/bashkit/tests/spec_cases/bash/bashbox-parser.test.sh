# BashBox parser cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_parser_until_loop
# until loop
i=0; until [[ $i -ge 3 ]]; do echo $i; i=$((i+1)); done > /tmp/o; cat /tmp/o
### expect
0
1
2
### end

### bashbox_parser_c_style_for_keeps_spacing_of_each_clause
# C-style for keeps spacing of each clause
for ((i = 3; i > 0; i = i - -1 - 2)); do echo $i; done 2>/dev/null
### expect
3
2
1
### end

### bashbox_parser_c_style_for_with_empty_condition
# C-style for with empty condition
for ((i=0; ; i++)); do [[ $i -eq 2 ]] && break; echo $i; done
### expect
0
1
### end

### bashbox_parser_group_redirection
# group redirection
{ echo a; echo b; } > /tmp/f; cat /tmp/f
### expect
a
b
### end

### bashbox_parser_fd_numbered_trailing_redirection
# fd-numbered trailing redirection
if true; then echo x >&2; fi 2>/dev/null; echo done
### expect
done
### end

### bashbox_parser_function_keyword
# function keyword
function f { echo f; }; function g() { echo g; }; f; g
### expect
f
g
### end

### bashbox_parser_comment_before_then
# comment before then
if true # c
then echo y; fi
### expect
y
### end

### bashbox_parser_newline_after_pipe
# newline after pipe
echo a |
 cat
### expect
a
### end

### bashbox_parser_background_command
# background command
true & echo bg
### expect
bg
### end

### bashbox_parser_time_p_is_not_a_command
# time -p is not a command
time -p echo hi
### expect
hi
### end

### bashbox_parser_parenthesised_alternation
# parenthesised alternation
case b in (a|b) echo ab;; *) echo other
esac
### expect
ab
### end

### bashbox_parser_newlines_between_items
# newlines between items
case x in
  a) echo a;;
  *) echo star;;
esac > /tmp/c; cat /tmp/c
### expect
star
### end

### bashbox_parser_fall_through_with
# fall through with ;&
case a in a) echo 1;& b) echo 2;; c) echo 3;; esac
### expect
1
2
### end

### bashbox_parser_continue_matching_with
# continue matching with ;;&
case ab in a*) echo 1;;& *b) echo 2;; *) echo 3;; esac
### expect
1
2
### end

### bashbox_parser_or_and_not_and_grouping
# or, and, not and grouping
[[ a == b || ( c == c && ! -z x ) ]] && echo y
### expect
y
### end

### bashbox_parser_bare_words_test_non_empty
# bare words test non-empty
[[ a ]] && echo y; [[ "" ]] || echo n
### expect
y
n
### end

### bashbox_parser_redirection_before_the_command_name
# redirection before the command name
>/tmp/f echo hi; cat /tmp/f
### expect
hi
### end

### bashbox_parser_fd_redirection_before_the_command_name
# fd redirection before the command name
2>/dev/null echo hi
### expect
hi
### end

### bashbox_parser_multi_line_array_with_comment
# multi-line array with comment
x=(a
 b # comment
 c); echo ${x[@]}
### expect
a b c
### end

### bashbox_parser_append_assignment
# append assignment
x=a; x+=b; echo $x
### expect
ab
### end

### bashbox_parser_braces_brackets_and_bang_are_words_outside_command_position
# braces, brackets and bang are words outside command position
echo } ]] !
### expect
} ]] !
### end

### bashbox_parser_a_number_apart_from_is_an_argument
# a number apart from > is an argument
echo 1 2 3 > /tmp/f; cat /tmp/f
### expect
1 2 3
### end

### bashbox_parser_a_number_touching_is_its_fd
# a number touching > is its fd
echo a 2>/tmp/f 3 >/tmp/g; cat /tmp/g
### expect
a 3
### end

### bashbox_parser_a_number_before_is_an_argument
# a number before &> is an argument
echo a 2&>/tmp/f; cat /tmp/f
### expect
a 2
### end

### bashbox_parser_name_before_names_the_fd
# {name} before > names the fd
exec {fd}>/tmp/f; echo hi >&$fd; cat /tmp/f
### expect
hi
### end

### bashbox_parser_here_string_reads_fd_0
# here-string reads fd 0
cat <<< hi
### expect
hi
### end

### bashbox_parser_array_elements_across_lines
# array elements across lines
x=(a

 b); echo ${x[1]}
### expect
b
### end

### bashbox_parser_group_and_alternation
# group and alternation
x=ab; [[ $x =~ ^(a|b)b$ ]] && echo y
### expect
y
### end

### bashbox_parser_blanks_inside_parentheses
# blanks inside parentheses
[[ "a b" =~ (a b) ]] && echo ${BASH_REMATCH[0]}
### expect
a b
### end

### bashbox_parser_ends_before
# ends before &&
[[ ab =~ a|c && x ]] && echo y
### expect
y
### end

# BashBox named-fd cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_named_fd_exec_opens_the_lowest_free_fd_from_10
# exec opens the lowest free fd from 10
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec {fd}>f; echo $fd; echo hi >&$fd; exec {fd}>&-; cat f
### expect
10
hi
### end

### bashbox_named_fd_fds_in_use_are_skipped
# fds in use are skipped
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec 10>x 11>y; exec {a}>f {b}>g; echo $a $b
### expect
12 13
### end

### bashbox_named_fd_a_regular_command_keeps_it_open
# a regular command keeps it open
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo hi {fd}>f; echo $fd; echo there >&$fd; cat f
### expect
hi
10
there
### end

### bashbox_named_fd_as_a_prefix
# as a prefix
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
{fd}>f echo x; echo $fd
### expect
x
10
### end

### bashbox_named_fd_read_from_a_named_fd
# read from a named fd
printf 'a\nb\n' > f; exec {fd}<f; read x <&$fd; read y <&$fd; echo $x $y
### expect
a b
### end

### bashbox_named_fd_here_string_and_here_document
# here-string and here-document
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec {h}<<<'line'; read x <&$h; echo "$x $h"; exec {d}<<E
doc
E
read y <&$d; echo "$y $d"
### expect
line 10
doc 11
### end

### bashbox_named_fd_read_write_and_append
# read-write and append
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec {fd}<>f; echo hi >&$fd; exec {a}>>f; echo more >&$a; cat f
### expect
hi
more
### end

### bashbox_named_fd_duplicating_fds
# duplicating fds
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec {fd}>&1; echo $fd; echo to-stdout >&$fd; exec {in}<&0; echo $in
### expect
10
to-stdout
11
### end

### bashbox_named_fd_a_closed_fd_is_reused
# a closed fd is reused
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec {fd}>f; echo $fd; exec {fd}>&-; exec {fd}>g; echo $fd
### expect
10
10
### end

### bashbox_named_fd_writing_to_a_closed_named_fd
# writing to a closed named fd
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec {fd}>f; exec {fd}>&-; echo hi >&$fd; echo $?
### expect
1
### end

### bashbox_named_fd_a_dup_to_a_word_is_ambiguous
# a dup to a word is ambiguous
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec {fd}>&f; echo "$? [$fd]"
### expect
1 []
### end

### bashbox_named_fd_a_readonly_variable
# a readonly variable
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
readonly fd=3; exec {fd}>f; echo $?
### expect
1
### end

### bashbox_named_fd_a_failed_open_leaves_the_variable_alone
# a failed open leaves the variable alone
exec {fd}>/nonexistent/x; echo "$? [$fd]"
### expect
1 []
### end

### bashbox_named_fd_into_a_local
# into a local
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local fd; exec {fd}>f; echo $fd; }; f; echo "[$fd]"
### expect
10
[]
### end

### bashbox_named_fd_into_an_array_element
# into an array element
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=(1 2); exec {x[1]}>f; echo ${x[@]}; exec {x[1]}>&-; echo $?
### expect
1 10
0
### end

### bashbox_named_fd_not_a_name_so_just_a_word
# not a name, so just a word
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo {1x}>f; cat f
### expect
{1x}
### end

### bashbox_named_fd_on_a_compound_command
# on a compound command
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
{ echo in; } {fd}>f; echo $fd; cat f
### expect
in
10
### end

### bashbox_named_fd_a_function_listing_keeps_the_names
# a function listing keeps the names
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ echo a {fd}>f; cat {x}<f 2>&1; read {y}<<<w; exec {z}>&-; echo {q}>&2; echo {r}>&$x; cat {h}<<E
x
E
}; declare -f f
### expect
f () 
{ 
    echo a {fd}> f;
    cat {x}< f 2>&1;
    read {y}<<< w;
    exec {z}>&-;
    echo {q}>&2;
    echo {r}>&$x;
    cat {h}<<E
x
E

}
### end

### bashbox_named_fd_a_numbered_here_string_and_here_document
# a numbered here-string and here-document
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec 3<<<hi; read x <&3; echo $x; cat 4<<E <&4
doc
E
### expect
hi
doc
### end

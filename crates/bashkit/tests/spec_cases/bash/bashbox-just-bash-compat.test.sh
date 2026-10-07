# BashBox just-bash-compat cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_just_bash_compat_read_consumes_stdin_line_by_line
# read consumes stdin line by line
printf "a\nb\n" | while read l; do echo "<$l>"; done
### expect
<a>
<b>
### end

### bashbox_just_bash_compat_loop_reads_redirected_file
# loop reads redirected file
printf "1\n2\n" > in; while read l; do echo $l | cat; done < in
### expect
1
2
### end

### bashbox_just_bash_compat_nested_reads_share_stdin
# nested reads share stdin
printf "1\n2\n" | { { read a; }; read b; echo $a$b; }
### expect
12
### end

### bashbox_just_bash_compat_read_returns_1_on_unterminated_last_line
# read returns 1 on unterminated last line
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf "x" | { read v; echo "$?:$v"; }
### expect
1:x
### end

### bashbox_just_bash_compat_pipe_into_if
# pipe into if
echo hi | if true; then cat; fi
### expect
hi
### end

### bashbox_just_bash_compat_loop_output_goes_through_the_pipe
# loop output goes through the pipe
for i in b a; do echo $i; done | sort
### expect
a
b
### end

### bashbox_just_bash_compat_loop_redirected_to_dev_null
# loop redirected to /dev/null
while true; do echo x; break; done >/dev/null; echo ok
### expect
ok
### end

### bashbox_just_bash_compat_group_redirected_to_file_with_2_1
# group redirected to file with 2>&1
{ echo o; echo e >&2; } > f 2>&1; cat f
### expect
o
e
### end

### bashbox_just_bash_compat_stderr_to_dev_null
# stderr to /dev/null
cat nope 2>/dev/null; echo rc=$?
### expect
rc=1
### end

### bashbox_just_bash_compat_stderr_into_the_pipe
# stderr into the pipe
ls nope 2>&1 | grep -c nope
### expect
1
### end

### bashbox_just_bash_compat_stdout_to_stderr
# stdout to stderr
echo e >&2 2>/dev/null; echo done
### expect
done
### end

### bashbox_just_bash_compat_missing_input_file_skips_the_command
# missing input file skips the command
cat < nofile 2>/dev/null; echo rc=$?
### expect
rc=1
### end

### bashbox_just_bash_compat_break_exits_loop_with_status_0
# break exits loop with status 0
while :; do false; break; done; echo $?
### expect
0
### end

### bashbox_just_bash_compat_bare_assignment_status
# bare assignment status
false; x=1; echo $?
### expect
0
### end

### bashbox_just_bash_compat_assignment_takes_substitution_status
# assignment takes substitution status
x=$(false); echo $?
### expect
1
### end

### bashbox_just_bash_compat_quoted_keeps_words
# quoted $@ keeps words
set -- "a b" c; printf "[%s]" "$@"; echo
### expect
[a b][c]
### end

### bashbox_just_bash_compat_empty_quoted_vanishes
# empty quoted $@ vanishes
set --; printf "[%s]" "$@" x; echo
### expect
[x]
### end

### bashbox_just_bash_compat_joins_with_surrounding_text
# $@ joins with surrounding text
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -- a b; printf "[%s]" "x$@y"; echo
### expect
[xa][by]
### end

### bashbox_just_bash_compat_quoted_array_expansion
# quoted array expansion
a=("x y" z); printf "[%s]" "${a[@]}" "${a[*]}"; echo
### expect
[x y][z][x y z]
### end

### bashbox_just_bash_compat_1_idiom
# ${1+"$@"} idiom
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -- "a b" c; for w in "${1+"$@"}"; do echo "[$w]"; done
### expect
[a b]
[c]
### end

### bashbox_just_bash_compat_positional_slice
# positional slice
set -- a b c; echo "${@:2}"
### expect
b c
### end

### bashbox_just_bash_compat_positional_beyond_9
# positional beyond 9
set -- 1 2 3 4 5 6 7 8 9 10 11; echo ${10} ${11}
### expect
10 11
### end

### bashbox_just_bash_compat_unquoted_variable_is_split
# unquoted variable is split
x="a  b"; printf "[%s]" $x "$x"; echo
### expect
[a][b][a  b]
### end

### bashbox_just_bash_compat_unquoted_empty_variable_vanishes
# unquoted empty variable vanishes
e=; printf "[%s]" $e "$e" x; echo
### expect
[][x]
### end

### bashbox_just_bash_compat_custom_ifs_splits_expansions_only
# custom IFS splits expansions only
IFS=:; x=a:b; printf "[%s]" $x; echo
### expect
[a][b]
### end

### bashbox_just_bash_compat_command_word_is_split
# command word is split
C="echo hi"; $C there
### expect
hi there
### end

### bashbox_just_bash_compat_default_and_alternative_operators
# default and alternative operators
unset u; e=; echo "[${u-d}][${u+a}][${e-d}][${e+a}][${e:-d}]"
### expect
[d][][][a][d]
### end

### bashbox_just_bash_compat_cannot_assign_to_positional
# cannot assign to positional
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
: ${1:=x}; echo after
### expect
### end

### bashbox_just_bash_compat_error_if_unset_stops_the_script
# error-if-unset stops the script
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo a; echo ${zz:?boom}; echo after
### expect
a
### end

### bashbox_just_bash_compat_glob_matches_directories_only_with
# glob matches directories only with */
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
mkdir d1 d2; touch f1; for d in */; do echo $d; done
### expect
d1/
d2/
### end

### bashbox_just_bash_compat_quoted_glob_stays_literal
# quoted glob stays literal
touch a.txt b.txt; echo *.txt "*.txt" \*.txt
### expect
a.txt b.txt *.txt *.txt
### end

### bashbox_just_bash_compat_glob_skips_dotfiles
# glob skips dotfiles
touch .h v; echo *
### expect
v
### end

### bashbox_just_bash_compat_glob_bracket_expression
# glob bracket expression
touch a1 a2 b1; echo a[12] [!a]1
### expect
a1 a2 b1
### end

### bashbox_just_bash_compat_glob_in_a_directory_component
# glob in a directory component
mkdir -p p/x p/y; touch p/x/f p/y/f; echo p/*/f
### expect
p/x/f p/y/f
### end

### bashbox_just_bash_compat_sed_append_keeps_leading_blanks
# sed append keeps leading blanks
echo x | sed "a\   indented"
### expect
x
   indented
### end

### bashbox_just_bash_compat_sed_addresses_and_commands
# sed addresses and commands
printf "1\n2\n3\n" | sed -n 2p; printf "1\n2\n3\n" | sed 2d; printf "a\nb\n" | sed "1c X"
### expect
2
1
3
X
b
### end

### bashbox_just_bash_compat_sed_range_and_negation
# sed range and negation
printf "x\nstart\ny\nend\nz\n" | sed -n "/start/,/end/p"; printf "1\n2\n3\n" | sed "\$!d"
### expect
start
y
end
3
### end

### bashbox_just_bash_compat_sed_multiple_commands
# sed multiple commands
echo abc | sed "s/a/A/;s/c/C/"
### expect
AbC
### end

### bashbox_just_bash_compat_sed_bre_groups_and_escaped_backslash
# sed BRE groups and escaped backslash
echo ab | sed "s/\(a\)\(b\)/\2\1/"; echo ab | sed "s/\(a\)/\\\\\1/"
### expect
ba
\ab
### end

### bashbox_just_bash_compat_sed_nth_occurrence
# sed nth occurrence
echo aaa | sed s/a/X/2; echo aaa | sed s/a/X/2g
### expect
aXa
aXX
### end

### bashbox_just_bash_compat_sed_n_and_hold_space
# sed N and hold space
printf "1\n2\n3\n4\n" | sed "N;s/\n/,/"; printf "1\n2\n" | sed -n "h;n;G;p"
### expect
1,2
3,4
2
1
### end

### bashbox_just_bash_compat_head_and_tail_n_shorthand
# head and tail -N shorthand
printf "1\n2\n3\n" | head -2; printf "1\n2\n3\n" | tail -2
### expect
1
2
2
3
### end

### bashbox_just_bash_compat_tr_octal_escapes
# tr octal escapes
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo a | tr a "\101"
### expect
A
### end

### bashbox_just_bash_compat_cp_f_and_n
# cp -f and -n
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
echo a > f; echo b > g; cp -f f h && cat h; cp -n g f; cat f
### expect
a
a
### end

### bashbox_just_bash_compat_yes_is_finite_in_a_pipe
# yes is finite in a pipe
yes | head -2; yes ab | head -1
### expect
y
y
ab
### end

### bashbox_just_bash_compat_realpath
# realpath
mkdir -p r/s; cd r; realpath s . | sed "s|.*/||"
### expect
s
r
### end

### bashbox_just_bash_compat_mktemp_creates_a_private_file
# mktemp creates a private file
f=$(mktemp); test -f "$f" && echo file; d=$(mktemp -d); test -d "$d" && echo dir
### expect
file
dir
### end

### bashbox_just_bash_compat_declare_a_compound_assignment_keeps_quoted_values
# declare -A compound assignment keeps quoted values
declare -A m=([k]="hello world" [j]=x); echo "${m[k]}|${m[j]}|${#m[@]}"
### expect
hello world|x|2
### end

### bashbox_just_bash_compat_local_a_compound_assignment
# local -A compound assignment
f(){ local -A m=([a]=1 [b]="2 3"); echo "${m[a]}${m[b]}"; }; f
### expect
12 3
### end

### bashbox_just_bash_compat_keyed_elements_in_an_indexed_array
# keyed elements in an indexed array
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
a=(x [5]=y z); echo ${!a[@]} ${a[6]}
### expect
0 5 6 z
### end

### bashbox_just_bash_compat_array_keys_are_expanded
# array keys are expanded
k=key; declare -A m=([$k]="v w"); echo "${m[key]}"
### expect
v w
### end

### bashbox_just_bash_compat_readonly_array_declaration
# readonly array declaration
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
readonly -a r=(1 2); echo ${r[1]}
### expect
2
### end

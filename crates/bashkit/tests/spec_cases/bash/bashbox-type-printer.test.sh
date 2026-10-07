# BashBox type-printer cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_type_printer_a_one_line_body
# a one-line body
f() { echo hi; }
type f
### expect
f is a function
f () 
{ 
    echo hi
}
### end

### bashbox_type_printer_assignments_words_and_redirections
# assignments, words and redirections
g() { a=1 b=2 cmd x 'y z' "q $v" >out 2>&1 <in; x=1 y= z+=2; }
type g
### expect
g is a function
g () 
{ 
    a=1 b=2 cmd x 'y z' "q $v" > out 2>&1 < in;
    x=1 y= z+=2
}
### end

### bashbox_type_printer_pipelines_and_or_lists_and_background_jobs
# pipelines, |&, !, and-or lists and background jobs
g() { echo a | grep b |& cat; ! true && false || echo x; sleep 1 & echo bg; }
type g
### expect
g is a function
g () 
{ 
    echo a | grep b 2>&1 | cat;
    ! true && false || echo x;
    sleep 1 & echo bg
}
### end

### bashbox_type_printer_a_trailing_and_a_list_of_background_jobs
# a trailing & and a list of background jobs
d() { a & }
e() { a &
b & }
type d e
### expect
d is a function
d () 
{ 
    a &
}
e is a function
e () 
{ 
    a & b &
}
### end

### bashbox_type_printer_if_elif_and_else_nest_like_bash_stores_them
# if, elif and else nest like bash stores them
h() { if a; then b; elif c; then d; else e; fi; }
type h
### expect
h is a function
h () 
{ 
    if a; then
        b;
    else
        if c; then
            d;
        else
            e;
        fi;
    fi
}
### end

### bashbox_type_printer_for_a_bare_for_while_and_until
# for, a bare for, while and until
h() { for i in 1 2; do echo $i; done; for j; do :; done; while x; do y; done; until x; do y; done; }
type h
### expect
h is a function
h () 
{ 
    for i in 1 2;
    do
        echo $i;
    done;
    for j in "$@";
    do
        :;
    done;
    while x; do
        y;
    done;
    until x; do
        y;
    done
}
### end

### bashbox_type_printer_case_with_alternatives_fallthrough_and_an_empty_body
# case with alternatives, fallthrough and an empty body
k() { case $1 in a|b) echo ab;; c) echo c;& d) ;;& *) echo def; esac; case x in (a) ;; esac; case w in esac; }
type k
### expect
k is a function
k () 
{ 
    case $1 in 
        a | b)
            echo ab
        ;;
        c)
            echo c
        ;&
        d)

        ;;&
        *)
            echo def
        ;;
    esac;
    case x in 
        a)

        ;;
    esac;
    case w in 
    esac
}
### end

### bashbox_type_printer_subshells_groups_arithmetic_and_conditionals
# subshells, groups, arithmetic and conditionals
k() { ( sub; shell ); { grp; }; (( x++ )); ((y=1)); [[ -n $a && ( $b == c* || ! -f x ) ]]; [[ x ]]; [[ ! ( a == b ) ]]; [[ (a) ]]; }
type k
### expect
k is a function
k () 
{ 
    ( sub;
    shell );
    { 
        grp
    };
    (( x++ ));
    ((y=1));
    [[ -n $a && ( $b == c* || ! -f x ) ]];
    [[ -n x ]];
    [[ ! ( a == b ) ]];
    [[ ( -n a ) ]]
}
### end

### bashbox_type_printer_c_style_for_keeps_each_part_as_written_empty_parts_read_1
# C-style for keeps each part as written, empty parts read 1
a() { for ((i=0;i<3;i++)); do echo; done; for (( i = 0 ; i < 3 ; i++ )); do :; done; for ((;;)); do break; done; }
type a
### expect
a is a function
a () 
{ 
    for ((i=0; i<3; i++))
    do
        echo;
    done;
    for ((i = 0 ; i < 3 ; i++ ))
    do
        :;
    done;
    for ((1; 1; 1))
    do
        break;
    done
}
### end

### bashbox_type_printer_a_subshell_body
# a subshell body
m() ( echo subshell-body )
type m
### expect
m is a function
m () 
{ 
    ( echo subshell-body )
}
### end

### bashbox_type_printer_a_compound_body_that_is_not_a_group
# a compound body that is not a group
f() if true; then :; fi
g() for i in a; do :; done 2>/dev/null
type f g
### expect
f is a function
f () 
{ 
    if true; then
        :;
    fi
}
g is a function
g () 
{ 
    for i in a;
    do
        :;
    done 2> /dev/null
}
### end

### bashbox_type_printer_here_documents_follow_the_line_that_opens_them
# here-documents follow the line that opens them
n() { cat <<EOF
hello $x
EOF
cat <<-'Q' > f
	lit
	Q
echo after; }
type n
### expect
n is a function
n () 
{ 
    cat <<EOF
hello $x
EOF

    cat <<-'Q' > f
lit
Q

    echo after
}
### end

### bashbox_type_printer_a_here_document_ending_the_body
# a here-document ending the body
c() { cat <<EOF
x
EOF
}
type c
### expect
c is a function
c () 
{ 
    cat <<EOF
x
EOF

}
### end

### bashbox_type_printer_a_here_document_in_a_pipeline
# a here-document in a pipeline
b() { cat <<EOF | grep x
body
EOF
echo z; }
type b
### expect
b is a function
b () 
{ 
    cat <<EOF |
body
EOF
  grep x
    echo z
}
### end

### bashbox_type_printer_here_documents_in_an_and_or_list
# here-documents in an and-or list
b() { cat <<"E1" && echo x || cat <<\E2
q
E1
r
E2
}
type b
### expect
b is a function
b () 
{ 
    cat <<'E1' && 
q
E1
 echo x || cat <<'E2'
r
E2

}
### end

### bashbox_type_printer_two_here_documents_on_one_command_and_the_missing_that_follo
# two here-documents on one command, and the missing ; that follows
z() { cat <<A <<'B' >f; echo hi
one
A
two
B
}
w() { cat <<EOF; echo a; echo b
x
EOF
}
type z w
### expect
z is a function
z () 
{ 
    cat <<A <<'B' > f
one
A
two
B

    echo hi
}
w is a function
w () 
{ 
    cat <<EOF
x
EOF

    echo a
    echo b
}
### end

### bashbox_type_printer_arrays_declarations_and_appends
# arrays, declarations and appends
function o { local x=(1 2 3) y; declare -A z=([a]=1); x+=(4); echo "${x[@]}"; x=( 1   2
 3 ); }
type o
### expect
o is a function
o () 
{ 
    local x=(1 2 3) y;
    declare -A z=([a]=1);
    x+=(4);
    echo "${x[@]}";
    x=(1 2 3)
}
### end

### bashbox_type_printer_time_substitutions_and_a_nested_function
# time, substitutions and a nested function
p() { time ls; time -p ls; echo $(date) `date` $((1+2)) ${a:-b}; i() { inner; }; }
type p
### expect
p is a function
p () 
{ 
    time ls;
    time -p ls;
    echo $(date) `date` $((1+2)) ${a:-b};
    function i () 
    { 
        inner
    }
}
### end

### bashbox_type_printer_redirections_of_the_function_itself
# redirections of the function itself
q() { echo a; } > /dev/null 2>&1
type q
### expect
q is a function
q () 
{ 
    echo a
} > /dev/null 2>&1
### end

### bashbox_type_printer_comments_and_continuations_disappear
# comments and continuations disappear
t() { echo # comment
}
v() {
  # leading comment
  echo one   # trailing
  echo "multi
line" \
  cont
}
type t v
### expect
t is a function
t () 
{ 
    echo
}
v is a function
v () 
{ 
    echo one;
    echo "multi
line" cont
}
### end

### bashbox_type_printer_quoting_is_kept_as_written
# quoting is kept as written
j() { echo a\ b "c\"d" 'e' \$x x=1; }
type j
### expect
j is a function
j () 
{ 
    echo a\ b "c\"d" 'e' \$x x=1
}
### end

### bashbox_type_printer_declare_f_prints_every_function_sorted
# declare -f prints every function, sorted
b() { :; }
a() { echo; }
declare -f
### expect
a () 
{ 
    echo
}
b () 
{ 
    :
}
### end

### bashbox_type_printer_declare_f_with_names_failing_quietly_for_a_missing_one
# declare -f with names, failing quietly for a missing one
b() { :; }
declare -f b nope; echo "s=$?"
### expect
b () 
{ 
    :
}
s=1
### end

### bashbox_type_printer_declare_f_lists_names
# declare -F lists names
b() { :; }
a() { :; }
declare -F; declare -F b nope a; echo "s=$?"
### expect
declare -f a
declare -f b
b
a
s=1
### end

### bashbox_type_printer_type_reports_each_name
# type reports each name
b() { :; }
type b nope; echo "s=$?"; type -t b
### expect
b is a function
b () 
{ 
    :
}
s=1
function
### end

# BashBox shopt cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_shopt_unknown_names_are_reported_but_the_rest_still_apply
# unknown names are reported but the rest still apply
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
shopt -s extglob nope nullglob; echo s=$?; shopt -p nullglob extglob; shopt -q nope; echo s=$?
### expect
s=1
shopt -s nullglob
shopt -s extglob
s=1
### end

### bashbox_shopt_bad_flags
# bad flags
shopt -s -u x; echo s=$?; shopt -x; echo s=$?; shopt -sx; echo s=$?
### expect
s=1
s=2
s=2
### end

### bashbox_shopt_o_with_a_bad_name
# -o with a bad name
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
shopt -so nope; echo s=$?; shopt -uo nope; echo s=$?; shopt -o nope; echo s=$?; shopt -po nope errexit; echo s=$?
### expect
s=0
s=0
s=1
set +o errexit
s=1
### end

### bashbox_shopt_lastpipe_is_accepted
# lastpipe is accepted
shopt -s lastpipe; echo a | read v; echo "[$v]"
### expect
[a]
### end

### bashbox_shopt_aliases_expand_only_with_expand_aliases_from_the_next_line_o
# aliases expand only with expand_aliases, from the next line on
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
alias e='echo hi'
e there
shopt -s expand_aliases
e there
alias echo='echo [e]'; echo x; x=1 e y > f; cat f
alias c='cat'; c <<EOF
body
EOF
"e" quoted; \e back
alias p='echo a | tr a b'; p
unalias echo; e z
alias echo='echo [e]'
e w; echo v; alias q=echo; eval q ev
f() { e inf; }; f
### expect
hi there
x
hi y
[e] hi z
[e] hi w
[e] v
[e] ev
[e] hi inf
### end

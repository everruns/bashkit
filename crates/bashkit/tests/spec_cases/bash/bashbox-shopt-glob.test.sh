# BashBox shopt-glob cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_shopt_glob_nullglob_drops_a_pattern_that_matches_nothing
# nullglob drops a pattern that matches nothing
mkdir -p d/e; touch a.txt b.txt .hid C.TXT d/x.txt d/e/y.txt
shopt -s nullglob; echo a *.nope b; x=(*.nope); echo ${#x[@]}; echo "*.nope"
### expect
a b
0
*.nope
### end

### bashbox_shopt_glob_dotglob_lets_match_dotfiles
# dotglob lets * match dotfiles
mkdir -p d/e; touch a.txt b.txt .hid C.TXT d/x.txt d/e/y.txt
shopt -s dotglob; echo *; echo .*; shopt -u dotglob; echo *; echo .*
### expect
.hid C.TXT a.txt b.txt d
.hid
C.TXT a.txt b.txt d
.hid
### end

### bashbox_shopt_glob_nocaseglob
# nocaseglob
mkdir -p d/e; touch a.txt b.txt .hid C.TXT d/x.txt d/e/y.txt
shopt -s nocaseglob; echo *.txt; echo [a-c]*; echo c.txt
### expect
C.TXT a.txt b.txt
C.TXT a.txt b.txt
c.txt
### end

### bashbox_shopt_glob_globstar_makes_recurse
# globstar makes ** recurse
mkdir -p d/e; touch a.txt b.txt .hid C.TXT d/x.txt d/e/y.txt
shopt -s globstar; echo **; echo **/*.txt; echo d/**; echo d/**/; shopt -u globstar; echo **/*.txt
### expect
C.TXT a.txt b.txt d d/e d/e/y.txt d/x.txt
a.txt b.txt d/e/y.txt d/x.txt
d/ d/e d/e/y.txt d/x.txt
d/ d/e/
d/x.txt
### end

### bashbox_shopt_glob_a_literal_last_part_must_exist
# a literal last part must exist
mkdir -p d/e; touch a.txt b.txt .hid C.TXT d/x.txt d/e/y.txt
echo */x.txt */nope.txt; echo d/*/; echo "*".txt \*.txt
### expect
d/x.txt */nope.txt
d/e/
*.txt *.txt
### end

### bashbox_shopt_glob_extglob_patterns_in_pathnames
# extglob patterns in pathnames
mkdir -p d/e; touch a.txt b.txt .hid C.TXT d/x.txt d/e/y.txt
shopt -s extglob
echo !(*.txt); echo @(a|b).txt; echo *(a).txt; echo +([a-c]).txt; echo ?(a).txt; echo !(d|*.txt|C*)
echo "@(a)".txt; x='@(a|b).txt'; echo $x; echo "$x"; echo @(nomatch)
### expect
C.TXT d
a.txt b.txt
a.txt
a.txt b.txt
a.txt
!(d|*.txt|C*)
@(a).txt
a.txt b.txt
@(a|b).txt
@(nomatch)
### end

### bashbox_shopt_glob_without_extglob_the_parentheses_are_literal
# without extglob the parentheses are literal
mkdir -p d/e; touch a.txt b.txt .hid C.TXT d/x.txt d/e/y.txt
x='@(a).txt'; echo $x
shopt -s extglob
echo $x
### expect
@(a).txt
a.txt
### end

# ${x#pat} / ${x%pat} match the full glob grammar: a lone `*` matches the
# empty string for the shortest forms, `?` matches one character, and several
# `*` combine.

### remove_lone_star
x=abc; e=
echo "${x#*}|${x%*}|${x##*}|${x%%*}|[${e#*}]"
### expect
abc|abc|||[]
### end

### remove_question_mark
x=abc
echo "${x#?}|${x%?}|${x#*?}|${x%?*}|${x##?}|${x#??}"
### expect
bc|ab|bc|ab|bc|c
### end

### remove_multi_star
p=/a/b.c/d.tar.gz
echo "${p#*/*/}|${p##*.*.}|${p%/*/*}|${p#/?/}|${p%%.*}|${p##*/}"
### expect
b.c/d.tar.gz|gz|/a|b.c/d.tar.gz|/a/b|d.tar.gz
### end

### remove_star_from_variable
y='a*b'; q='*'
echo "${y#$q}|${y#"$q"}|${y%$q}"
### expect
a*b|a*b|a*b
### end

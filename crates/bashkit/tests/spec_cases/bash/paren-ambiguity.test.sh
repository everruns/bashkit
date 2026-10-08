# `((` that does not close with `))` is two nested subshells, and `$((`
# likewise a command substitution of a subshell, as bash reads them.

### dparen_closed_by_separate_parens_is_subshell
((echo a) ; echo b)
echo st=$?
### expect
a
b
st=0
### end

### dparen_negated_test_subshells
i=-g
if ! ((test x"$i" = x-g) || (test x"$i" = x-O2)); then echo no; else echo yes; fi
### expect
yes
### end

### dollar_dparen_closed_separately_is_command_subst
echo $((echo 1; echo 2) )
### expect
1 2
### end

### backtick_starting_with_paren
x=`(echo a; echo b) | head -n 1`
echo $x
### expect
a
### end

### arithmetic_still_arithmetic
echo $(( (1+2)*3 ))
(( (2+3) == 5 )) && echo ok
### expect
9
ok
### end

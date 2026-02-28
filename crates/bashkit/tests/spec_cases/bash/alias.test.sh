# Alias tests
# Inspired by Oils spec/alias.test.sh
# https://github.com/oilshell/oil/blob/master/spec/alias.test.sh

### alias_basic
# Basic alias definition and use

shopt -s expand_aliases
alias hi='echo hello world'
hi
### expect
hello world
### end

### alias_override_builtin
# alias can override builtin

shopt -s expand_aliases
alias echo='echo foo'
echo bar
### expect
foo bar
### end

### alias_define_multiple
# defining multiple aliases

shopt -s expand_aliases
alias echo_x='echo X' echo_y='echo Y'
echo_x
echo_y
### expect
X
Y
### end

### alias_unalias
# unalias removes alias

shopt -s expand_aliases
alias hi='echo hello'
hi
unalias hi
hi 2>/dev/null
echo status=$?
### expect
hello
status=127
### end

### alias_unalias_all
# unalias -a removes all

alias foo=bar
alias spam=eggs
unalias -a
alias 2>/dev/null | wc -l
### expect
0
### end

### alias_not_defined_error
# alias for non-existent returns error

alias nonexistentZZZ 2>/dev/null
echo status=$?
### expect
status=1
### end

### alias_unalias_not_defined_error
# unalias for non-existent returns error

unalias nonexistentZZZ 2>/dev/null
echo status=$?
### expect
status=1
### end

### alias_with_variable
# Alias with variable expansion at use-time
### skip: TODO lexer loses single-quote context in mid-word tokens (echo_x='echo $x' expands $x at parse time)
shopt -s expand_aliases
x=early
alias echo_x='echo $x'
x=late
echo_x
### expect
late
### end

### alias_trailing_space
# alias with trailing space triggers expansion of next word

shopt -s expand_aliases
alias hi='echo hello world '
alias punct='!!!'
hi punct
### expect
hello world !!!
### end

### alias_recursive_first_word
# Recursive alias expansion of first word

shopt -s expand_aliases
alias hi='e_ hello world'
alias e_='echo __'
hi
### expect
__ hello world
### end

### alias_must_be_unquoted
# Alias must be an unquoted word

shopt -s expand_aliases
alias echo_alias_='echo'
cmd=echo_alias_
echo_alias_ X
$cmd X 2>/dev/null
echo status=$?
### expect
X
status=127
### end

### alias_in_pipeline
# Two aliases in pipeline

shopt -s expand_aliases
alias myseq='seq '
alias mywc='wc '
myseq 3 | mywc -l
### expect
3
### end

### alias_used_in_subshell
# alias used in subshell

shopt -s expand_aliases
alias echo_='echo [ '
( echo_ subshell; )
echo $(echo_ commandsub)
### expect
[ subshell
[ commandsub
### end

### alias_with_semicolon_pipeline
# Alias that is && || ;

shopt -s expand_aliases
alias t1='echo one && echo two'
t1
### expect
one
two
### end

### alias_list_all
# alias without args lists all

alias ex=exit ll='ls -l'
alias | grep -c 'ex\|ll'
### expect
2
### end

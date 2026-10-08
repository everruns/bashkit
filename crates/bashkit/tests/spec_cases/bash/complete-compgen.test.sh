# Programmable completion builtins: complete, compgen, compopt.

### complete_prints_specs
complete
complete -W 'foo bar' mycommand
complete -p
complete -F myfunc other
complete -W "it's" q
complete -p other q
### expect
complete -W 'foo bar' mycommand
complete -F myfunc other
complete -W 'it'\''s' q
### end

### complete_full_spec_print_order
complete -o nospace -o default -A function -a -v -G '*.c' -W 'a b' -X '!x' -P pre -S suf -F fn -C cmd -A setopt x
complete -p x
### expect
complete -o default -o nospace -a -v -A function -A setopt -G '*.c' -W 'a b' -P 'pre' -S 'suf' -X '!x' -C 'cmd' -F fn x
### end

### complete_default_empty_initial
complete -D -F f
complete -E -W e
complete -p -D
complete -p -E
### expect
complete -F f -D
complete -W 'e' -E
### end

### complete_usage_errors
complete -F f
echo status=$?
complete -p nosuch
echo status=$?
complete -r nosuch
echo status=$?
complete foo
echo status=$?
complete -r foo
complete -p
echo done
### expect
status=2
status=1
status=0
status=0
done
### end

### compgen_invalid_action_and_option
compgen -A foo
echo status=$?
compgen -o bogus -W x
echo status=$?
### expect
status=2
status=2
### end

### compopt_outside_completion
compopt -o invalid
echo status=$?
compopt -o filenames +o nospace
echo status=$?
complete -W x c1
compopt -o nospace c1
complete -p c1
### expect
status=2
status=1
complete -o nospace -W 'x' c1
### end

### compgen_function_call_and_comp_vars
f() {
  echo "args:$1|$2|$3"
  echo "cword:$COMP_CWORD line:[$COMP_LINE] point:$COMP_POINT words:${#COMP_WORDS[@]}"
  COMPREPLY=(one two three)
}
compgen -F f foo a b
echo "after:${COMP_CWORD-unset}:${COMPREPLY-unset}"
### expect
args:compgen|foo|
cword:-1 line:[] point:0 words:0
one
two
three
after:unset:unset
### end

### compgen_function_scalar_reply_and_filter
g() { COMPREPLY=hello; }
compgen -F g
h() { COMPREPLY=(one two three bin); }
shopt -s extglob
compgen -X '@(two|bin)' -F h
echo --
compgen -X '!@(two|bin)' -F h
### expect
hello
one
three
--
two
bin
### end

### compgen_command_option
f() { echo foo; echo "bar:$*"; }
compgen -C f b
echo status=$?
### expect
foo
bar:compgen b 
status=0
### end

### compgen_wordlist_order_prefix_suffix
compgen -W 'one two three'
echo --
compgen -W 'aa ab b' -P '<' -S '>' a
echo --
compgen -W 'aa ab' -X '&b' a
### expect
one
two
three
--
<aa>
<ab>
--
aa
### end

### compgen_wordlist_expands_and_splits_on_ifs
IFS=':%'
compgen -W '$(echo "spam:eggs%ham cheese")'
compgen -W 'x:y%z w\:v'
### expect
spam
eggs
ham cheese
x
y
z w:v
### end

### compgen_wordlist_errors
compgen -W '${'
echo status=$?
compgen -W 'foo $(( 1 / 0 )) bar'
echo status=$?
compgen -W '' -- foo
echo status=$?
### expect
status=1
status=1
status=1
### end

### compgen_keywords
compgen -k | tr '\n' ' '
echo
compgen -k el
### expect
if then else elif fi case esac for select while until do done in function time { } ! [[ ]] coproc 
else
elif
### end

### compgen_actions_order
mkdir -p /tmp/cga && cd /tmp/cga && touch QZ_FILE Q_FILE
QZ_FUNC() { :; }
QZX=1
compgen -A function -A variable -A file QZ
### expect
QZ_FUNC
QZX
QZ_FILE
### end

### compgen_alias_setopt_shopt_helptopic
alias v_alias=ls v_alias2=ls a1=ls
compgen -A alias -A setopt v
compgen -A shopt -P '[' -S ']' nu
compgen -A helptopic -S ___ fal
compgen -A builtin getop
compgen -A command whil
compgen -A signal SIGHU
### expect
v_alias
v_alias2
verbose
vi
[nullglob]
false___
getopts
while
SIGHUP
### end

### compgen_exported_variables
export v1_global=0
unexported=1
f() { local v2_local=0; export v2_local; compgen -e v; }
f
compgen -e unexported
echo status=$?
### expect
v1_global
v2_local
status=1
### end

### compgen_files_and_dirs
mkdir -p /tmp/cgf/sub /tmp/cgf/sdir && cd /tmp/cgf && touch spam.py spam.sh sub/x1 sub/x2 'foo bar'
compgen -f sp | sort
compgen -d s | sort
compgen -f sub/ | sort
compgen -f 'foo b'
compgen -f -X '!*.py' sp
compgen -f /nonexistent/
echo status=$?
### expect
spam.py
spam.sh
sdir
sub
sub/x1
sub/x2
foo bar
spam.py
status=1
### end

### compgen_dir_options
mkdir -p /tmp/cgo/bin /tmp/cgo/build && cd /tmp/cgo && touch bfile
compgen -o plusdirs -W 'a b1' b | sort
echo --
compgen -o dirnames b | sort
echo --
compgen -o default bf
echo --
compgen -o filenames -o nospace -W 'x y'
### expect
b1
bin
build
--
bin
build
--
bfile
--
x
y
### end

### dotglob_off_default
# By default, * does not match dotfiles
mkdir -p /tmp/dg_test
cd /tmp/dg_test
touch a b .hidden
echo *
### expect
a b
### end

### dotglob_on
# With dotglob, * matches dotfiles
### bash_diff
mkdir -p /tmp/dg_on
cd /tmp/dg_on
touch a b .hidden
shopt -s dotglob
echo *
### expect
.hidden a b
### end

### dotglob_explicit_dot
# Pattern starting with . always matches dotfiles regardless of dotglob
mkdir -p /tmp/dg_dot
cd /tmp/dg_dot
touch .foo .bar visible
echo .*
### expect
.bar .foo
### end

### nocaseglob_off
# Without nocaseglob, glob is case-sensitive
mkdir -p /tmp/ncg_off
cd /tmp/ncg_off
touch ABC abc
echo [a]*
### expect
abc
### end

### nocaseglob_on
# With nocaseglob, glob is case-insensitive
### bash_diff
mkdir -p /tmp/ncg_on
cd /tmp/ncg_on
touch ABC abc
shopt -s nocaseglob
echo [a]*
### expect
ABC abc
### end

### nullglob_off
# Without nullglob, unmatched glob stays literal
mkdir -p /tmp/ng_off
cd /tmp/ng_off
echo *.nonexistent
### expect
*.nonexistent
### end

### nullglob_on
# With nullglob, unmatched glob expands to nothing
mkdir -p /tmp/ng_on
cd /tmp/ng_on
shopt -s nullglob
for f in *.nonexistent; do echo "got: $f"; done
echo "done"
### expect
done
### end

### failglob_on
# With failglob, unmatched glob is an error
### bash_diff
mkdir -p /tmp/fg_on
cd /tmp/fg_on
shopt -s failglob
echo *.nonexistent 2>/dev/null
echo "exit:$?"
### expect
exit:1
### end

### noglob_set_f
# set -f disables glob expansion
mkdir -p /tmp/ng_setf
cd /tmp/ng_setf
touch a b c
set -f
echo *
### expect
*
### end

### noglob_restored
# set +f re-enables glob expansion
mkdir -p /tmp/ng_restore
cd /tmp/ng_restore
touch x y z
set -f
echo *
set +f
echo *
### expect
*
x y z
### end

### dotglob_toggle
# dotglob can be toggled off
### bash_diff
mkdir -p /tmp/dg_toggle
cd /tmp/dg_toggle
touch a .hidden
shopt -s dotglob
echo *
shopt -u dotglob
echo *
### expect
.hidden a
a
### end

### globstar_off_default
# Without globstar, ** is treated as regular *
mkdir -p /tmp/gs_off/sub
touch /tmp/gs_off/top.txt /tmp/gs_off/sub/deep.txt
cd /tmp/gs_off
echo **
### expect
sub top.txt
### end

### globstar_on
# With globstar, ** matches recursively
### bash_diff
mkdir -p /tmp/gs_on/sub
touch /tmp/gs_on/top.txt /tmp/gs_on/sub/deep.txt
shopt -s globstar
echo /tmp/gs_on/**/*.txt
### expect
/tmp/gs_on/sub/deep.txt /tmp/gs_on/top.txt
### end

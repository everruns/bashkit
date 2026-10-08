# Redirect targets, here-docs on numbered fds, and type/hash/eval/source/
# shopt/test/option variables. Written for bashkit; expected output checked
# against GNU bash 5.2.

### type_finds_relative_path_entry
cd /tmp/; mkdir -p tb; printf 'echo hi\n' > tb/tool; chmod +x tb/tool
PATH=tb:$PATH type tool
PATH=tb:$PATH type -P tool
### expect
tool is tb/tool
tb/tool
### end

### type_reports_hashed_after_use
cd /tmp/; mkdir -p tc; printf 'echo run\n' > tc/runme; chmod +x tc/runme
PATH=/tmp/tc:$PATH
runme
type runme
hash
### expect
run
runme is hashed (/tmp/tc/runme)
hits	command
   2	/tmp/tc/runme
### end

### hash_empty_and_reset_on_path_change
hash; echo s=$?
cd /tmp/; mkdir -p td; printf 'echo x\n' > td/xx; chmod +x td/xx
PATH=/tmp/td:$PATH; xx >/dev/null; PATH=$PATH; hash; echo s=$?
### expect
hash: hash table empty
s=0
hash: hash table empty
s=0
### end

### eval_and_source_accept_double_dash
eval -- 'echo e'
echo 'echo s' > /tmp/s1.sh
source -- /tmp/s1.sh
eval -x 'echo no' 2>/dev/null; echo s=$?
### expect
e
s
s=2
### end

### shopt_status_and_defaults
shopt -q sourcepath; echo s=$?
shopt -q extglob; echo s=$?
shopt sourcepath extglob >/dev/null; echo s=$?
shopt -p globskipdots
shopt -u globskipdots; shopt -p globskipdots
### expect
s=0
s=1
s=1
shopt -s globskipdots
shopt -u globskipdots
### end

### test_v_and_o
x=1
test -v x && echo set
[ -v nope ] || echo unset
set -o errexit; set +o errexit; set -o noglob
[ -o noglob ] && echo on
test -o errexit || echo off
### expect
set
unset
on
off
### end

### shellopts_and_bashopts_track_options
set -o noglob
case :$SHELLOPTS: in *:noglob:*) echo yes;; esac
shopt -s extglob
case :$BASHOPTS: in *:extglob:*) echo yes;; esac
bash -c 'SHELLOPTS=x; echo not-reached' 2>/dev/null; echo s=$?
### expect
yes
yes
s=1
### end

### export_shellopts_reaches_child_shell
export SHELLOPTS
set -o noglob
bash -c 'case $- in *f*) echo inherited;; esac'
### expect
inherited
### end

### bash_c_option_parsing
bash -c -e 'echo $-' | grep -q e && echo e-on
bash -O extglob -c 'shopt -q extglob && echo ext'
bash -c 'echo "$0 $1"' zero one
bash -- -c 2>/dev/null; echo s=$?
### expect
e-on
ext
zero one
s=127
### end

### stderr_merge_keeps_order
f() { echo out; echo err >&2; echo out2; }
f 2>&1 | cat
{ echo a; echo b >&2; } 2>&1 | cat
### expect
out
err
out2
a
b
### end

### ambiguous_and_glob_redirect_targets
cd /tmp/; rm -rf rt; mkdir rt; cd rt
v='a b'
echo x > $v; echo s=$?
echo y > "b*"; ls
touch c1 c2; echo z > c*; echo s=$?
### expect
s=1
b*
s=1
### end

### dup_input_to_output_fd
{ echo to-err 1<&2; } 2>&1 | cat
### expect
to-err
### end

### exec_move_fd
cd /tmp/
exec 5>mv.txt
exec 6>&5-
echo moved >&6
echo closed >&5 2>/dev/null || echo gone
exec 6>&-
cat mv.txt
### expect
gone
moved
### end

### heredoc_on_numbered_fd_for_function
f() { cat <&4; }
f 4<<EOF
four
EOF
read -r line 3<<<three <&3; echo $line
### expect
four
three
### end

### heredoc_line_continuation
cat <<EOF
one \
two
EOF
cat <<'EOF'
keep \
lines
EOF
### expect
one two
keep \
lines
### end

### noclobber_refuses_ampersand_redirect
cd /tmp/; echo old > nc.txt
set -C
echo new &> nc.txt; echo s=$?
cat nc.txt
### expect
s=1
old
### end

### bashpid_differs_in_subshell
[ "$BASHPID" = "$$" ] && echo same
( [ "$BASHPID" != "$$" ] && echo differs )
### expect
same
differs
### end

### underscore_tracks_last_argument
echo a b >/dev/null; echo $_
x=1; echo "[$_]"
declare -a arr=(1 2); echo $_
### expect
b
[]
arr
### end

### wc_width_follows_input_size
cd /tmp/; printf 'a b\n' > w1.txt
wc -l w1.txt
wc w1.txt
### expect
1 w1.txt
1 2 4 w1.txt
### end

### env_i_child_has_startup_variables
env -i bash -c 'echo "$PS4|${PATH:+path}|${PWD:+pwd}"'
### expect
+ |path|pwd
### end

### lineno_in_arith_for_is_the_for_line
for (( i = 0; i < LINENO; i++ )); do
  echo $i
done
### expect
0
### end

### temp_path_assignment_hashes_nothing
cd /tmp/; mkdir -p te; printf 'echo te\n' > te/tecmd; chmod +x te/tecmd
PATH=/tmp/te:$PATH tecmd
hash
### expect
te
hash: hash table empty
### end

### dup_input_from_unopened_fd_fails
cat <&7; echo s=$?
### expect
s=1
### end

# `bash -i` children: rc files, PROMPT_COMMAND, $- and history files.

### interactive_rcfile_with_c
echo 'echo RCFILE' > /tmp/ic_rc
bash --rcfile /tmp/ic_rc -i -c 'echo 2' 2>/dev/null
bash --norc --rcfile /tmp/ic_rc -i -c 'echo 3' 2>/dev/null
### expect
RCFILE
2
3
### end

### interactive_rcfile_exit
printf 'echo one\nexit 42\necho two\n' > /tmp/ic_rc2
bash --rcfile /tmp/ic_rc2 -i -c 'echo hello' 2>/dev/null
echo status=$?
### expect
one
status=42
### end

### interactive_rcfile_parse_error
echo 'foo >' > /tmp/ic_bad
bash --rcfile /tmp/ic_bad -i -c 'echo hi' 2>/tmp/ic_err
echo status=$?
grep -q 'ic_bad' /tmp/ic_err && echo named
### expect
hi
status=0
named
### end

### interactive_prompt_command
export HISTFILE=/tmp/ic_hist
bash --norc -i 2>/dev/null <<'EOF2'
f() { echo "last=$?"; }
PROMPT_COMMAND=f
( exit 42 )
echo ok
EOF2
### expect
last=0
last=42
ok
last=0
### end

### interactive_flags
bash --norc -i -c 'case $- in *i*) echo interactive;; esac' 2>/dev/null
bash -c 'case $- in *i*) echo interactive;; *) echo not;; esac'
### expect
interactive
not
### end

### interactive_histfile_round_trip
cd /tmp
echo 'echo 1' > ic_my
printf 'echo 2\nhistory\n' | HISTFILE=ic_my bash --norc -i 2>/dev/null
cat ic_my
### expect
2
    1  echo 1
    2  echo 2
    3  history
echo 1
echo 2
history
### end

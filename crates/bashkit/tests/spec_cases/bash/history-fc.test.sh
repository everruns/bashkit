# bash history: the history and fc builtins, $HISTFILE, $HISTSIZE,
# $HISTFILESIZE, and line recording in `bash -i` and with `set -o history`.

### history_bash_i_records_lines
export HISTFILE=/tmp/hf1
rm -f $HISTFILE
bash --norc -i 2>/dev/null <<'EOF2'
echo one >/dev/null
echo two; echo three >/dev/null
history
EOF2
### expect
two
    1  echo one >/dev/null
    2  echo two; echo three >/dev/null
    3  history
### end

### history_clear_then_count
export HISTFILE=/tmp/hf2
rm -f $HISTFILE
bash --norc -i 2>/dev/null <<'EOF2'
echo foo >/dev/null
history -c
history | wc -l
EOF2
### expect
1
### end

### history_file_ops
cd /tmp
printf 'cmd orig%s\n' 1 2 3 > hf3
HISTFILE=hf3
history -c
history -r
history -r
history | grep -c orig
echo 'cmd old' > hf3
history -w
grep -c old hf3
history -r nonexistent-file
echo status=$?
### expect
6
0
status=1
### end

### history_n_reads_new_lines
cd /tmp
printf 'cmd orig%s\n' 1 2 > hfa
cp hfa hfb
printf 'cmd new%s\n' 1 2 3 >> hfb
history -c
HISTFILE=hfa history -r
HISTFILE=hfb history -n
history | grep -c orig
history | grep -c new
### expect
2
3
### end

### history_append_a
cd /tmp
rm -f hf4
HISTFILE=hf4 bash --norc -i 2>/dev/null <<'EOF2'
history -c
echo 1
history -a
cat hf4
EOF2
### expect
1
echo 1
history -a
### end

### history_delete
export HISTFILE=/tmp/hf5
rm -f $HISTFILE
bash --norc -i 2>/dev/null <<'EOF2'
echo 42 >/dev/null
echo 43 >/dev/null
history -d 1
echo status=$?
history -d -1
echo status=$?
history -d 99
echo status=$?
history
EOF2
### expect
status=0
status=0
status=1
    1  echo 43 >/dev/null
    2  history -d 1
    3  echo status=$?
    4  echo status=$?
    5  history -d 99
    6  echo status=$?
    7  history
### end

### history_usage_errors
history not-a-number
echo status=$?
### expect
status=1
### end

### history_size_stifles
cd /tmp
printf 'cmd %s\n' 1 2 3 4 5 6 > hf6
HISTFILE=hf6
history -c
history -r
history | wc -l
HISTSIZE=2
history | sed 's/^ *[0-9]*  //'
### expect
6
cmd 5
cmd 6
### end

### history_filesize_truncates
cd /tmp
printf 'cmd %s\n' 1 2 3 4 5 6 > hf7
HISTFILE=hf7
HISTFILESIZE=2
cat hf7
### expect
cmd 5
cmd 6
### end

### history_s_and_p
export HISTFILE=/tmp/hf8
rm -f $HISTFILE
bash --norc -i 2>/dev/null <<'EOF2'
history -c
history -s stored line
history -p a b
history
EOF2
### expect
a
b
    1  stored line
    2  history
### end

### history_set_o_toggle
cd /tmp
printf 'echo %s\n' 1 > hf9
HISTFILE=hf9 bash --norc -i 2>/dev/null <<'EOF2'
set +o history
echo "not recorded" >/dev/null
set -o history
echo "recorded" >/dev/null
EOF2
grep -c "not recorded" hf9
grep -c "recorded" hf9
### expect
0
1
### end

### history_histappend_keeps_file
cd /tmp
export HISTFILE=hf10 HISTSIZE=10
printf 'cmd orig%s\n' 1 2 3 > hf10
bash --norc -i 2>/dev/null <<'EOF2'
HISTSIZE=1
shopt -s histappend
echo new >/dev/null
EOF2
grep -c orig hf10
printf 'cmd orig%s\n' 1 2 3 > hf10
bash --norc -i 2>/dev/null <<'EOF2'
HISTSIZE=1
echo new >/dev/null
EOF2
grep -c orig hf10
### expect
3
0
### end

### fc_list_forms
cd /tmp
printf 'echo %s\n' 1 2 3 > hf11
HISTFILE=hf11 bash --norc -i 2>/dev/null <<'EOF2'
history -c
history -r
fc -l
fc -ln 2 3
fc -lr -3 -2
fc -l 3 2
fc -l ech
EOF2
### expect
1	 history -r
2	 echo 1
3	 echo 2
4	 echo 3
	 echo 1
	 echo 2
5	 fc -l
4	 echo 3
3	 echo 2
2	 echo 1
4	 echo 3
5	 fc -l
6	 fc -ln 2 3
7	 fc -lr -3 -2
8	 fc -l 3 2
### end

### fc_rerun
export HISTFILE=/tmp/hf12
rm -f $HISTFILE
bash --norc -i 2>/dev/null <<'EOF2'
echo hello
fc -s hello=bye
fc -e - echo
EOF2
### expect
hello
bye
bye
### end

### fc_without_history
fc -l
echo status=$?
fc -l 0 1 2
echo status=$?
### expect
status=0
status=0
### end

### prompt_history_and_command_numbers
set -o history
PS1='\!'
history -c
echo "${PS1@P}"
### bash_diff: bash -c never records lines; bashkit records a script's lines with set -o history
### expect
1
### end

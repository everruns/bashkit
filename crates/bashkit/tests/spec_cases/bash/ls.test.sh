### ls_file_preserves_path
# ls with file arguments should preserve the full path in output
mkdir -p /tmp/lsdir
echo x > /tmp/lsdir/a.md
echo y > /tmp/lsdir/b.md
ls /tmp/lsdir/a.md /tmp/lsdir/b.md
### expect
/tmp/lsdir/a.md
/tmp/lsdir/b.md
### end

### ls_file_preserves_path_sorted_by_time
# ls -t with file arguments should preserve the full path in output
mkdir -p /tmp/lstdir
echo x > /tmp/lstdir/a.md
sleep 0.01
echo y > /tmp/lstdir/b.md
ls -t /tmp/lstdir/a.md /tmp/lstdir/b.md
### expect
/tmp/lstdir/b.md
/tmp/lstdir/a.md
### end

### ls_directory_shows_filenames_only
# ls on a directory should show filenames only, not full paths
mkdir -p /tmp/lsdironly
echo x > /tmp/lsdironly/file1.txt
echo y > /tmp/lsdironly/file2.txt
ls /tmp/lsdironly
### expect
file1.txt
file2.txt
### end

### ls_single_file_preserves_path
# ls with a single file argument should preserve the full path
mkdir -p /tmp/lssingle
echo x > /tmp/lssingle/test.txt
ls /tmp/lssingle/test.txt
### expect
/tmp/lssingle/test.txt
### end

### ls_classify_directory
# ls -F should append / to directories
mkdir -p /tmp/lsclass/subdir
echo x > /tmp/lsclass/file.txt
ls -F /tmp/lsclass
### expect
file.txt
subdir/
### end

### ls_classify_executable
# ls -F should append * to executable files
mkdir -p /tmp/lsexec
echo x > /tmp/lsexec/script.sh
chmod 755 /tmp/lsexec/script.sh
echo y > /tmp/lsexec/data.txt
ls -F /tmp/lsexec
### expect
data.txt
script.sh*
### end

### ls_classify_file_arg
# ls -F with file argument should append indicator
mkdir -p /tmp/lscf
mkdir -p /tmp/lscf/mydir
echo x > /tmp/lscf/normal.txt
ls -F /tmp/lscf/mydir /tmp/lscf/normal.txt
### expect
/tmp/lscf/normal.txt

/tmp/lscf/mydir:
### end

### ls_columns_basic
# ls -C should produce multi-column output
mkdir -p /tmp/lscol
touch /tmp/lscol/alpha /tmp/lscol/beta /tmp/lscol/delta /tmp/lscol/gamma
ls -C /tmp/lscol
### expect
alpha  beta  delta  gamma
### end

### ls_columns_with_classify
# ls -CF should combine classify and columns
mkdir -p /tmp/lscf2/subdir
touch /tmp/lscf2/file.txt
ls -CF /tmp/lscf2
### expect
file.txt  subdir/
### end

### ls_one_per_line_overrides_columns
# ls -1 should override -C (one per line)
mkdir -p /tmp/ls1c
touch /tmp/ls1c/aaa /tmp/ls1c/bbb /tmp/ls1c/ccc
ls -C1 /tmp/ls1c
### expect
aaa
bbb
ccc
### end

### ls_classify_long
### bash_diff: bashkit ls -l omits 'total' line
# ls -lF should append indicators in long format
mkdir -p /tmp/lslf
mkdir -p /tmp/lslf/sub
echo x > /tmp/lslf/file.txt
ls -lF /tmp/lslf | grep -v "^total" | awk '{print $NF}'
### expect
file.txt
sub/
### end

### ls_quote_name_short_flag
### bash_diff: bashkit accepts -Q via the uu_ls argument surface but does
### bash_diff: not yet implement quoting; report not-yet-impl rather than
### bash_diff: silently fall through to default rendering.
mkdir -p /tmp/lsqn
echo x > /tmp/lsqn/a.txt
ls -Q /tmp/lsqn 2>&1
echo "exit=$?"
### expect
ls: option(s) not yet implemented in bashkit: quote-name
exit=2
### end

### ls_quoting_style_long_flag
### bash_diff: --quoting-style= parsed by clap but not implemented.
mkdir -p /tmp/lsqs
echo x > /tmp/lsqs/a.txt
ls --quoting-style=shell /tmp/lsqs 2>&1
echo "exit=$?"
### expect
ls: option(s) not yet implemented in bashkit: quoting-style
exit=2
### end

### ls_group_directories_first_flag
### bash_diff: --group-directories-first parsed by clap but not implemented.
mkdir -p /tmp/lsgdf/sub
echo x > /tmp/lsgdf/file.txt
ls --group-directories-first /tmp/lsgdf 2>&1
echo "exit=$?"
### expect
ls: option(s) not yet implemented in bashkit: group-directories-first
exit=2
### end

### ls_composite_unsupported_then_supported
### bash_diff: -lr (long + reverse) — uu_ls treats -r as --reverse, which
### bash_diff: bashkit doesn't render yet. The unsupported-flag check
### bash_diff: fires before the listing runs.
mkdir -p /tmp/lscomp
echo x > /tmp/lscomp/a.txt
ls -lr /tmp/lscomp 2>&1
echo "exit=$?"
### expect
ls: option(s) not yet implemented in bashkit: reverse
exit=2
### end

### ls_unknown_flag_rejected
### bash_diff: clap-rendered diagnostic differs from GNU's "invalid option"
### bash_diff: wording, but exit code matches (2).
ls -z >/dev/null 2>&1
echo "exit=$?"
### expect
exit=2
### end

### ls_version
### bash_diff: bashkit reports its own version string (clap default)
ls --version | sed -E 's/^(ls) [0-9]+\.[0-9]+\.[0-9]+.*/\1 X.Y.Z/'
### expect
ls X.Y.Z
### end

### mktemp_creates_file
# mktemp prints the path of a newly-created file under /tmp.
# Default template is `tmp.` + 10 random chars (matches the bashkit
# behaviour, divergent from GNU's 6-char default).
p=$(mktemp)
case "$p" in
  /tmp/tmp.??????????) echo ok ;;
  *) echo "bad: $p" ;;
esac
test -f "$p" && echo present
### expect
ok
present
### end

### mktemp_directory
# -d creates a directory rather than a file
p=$(mktemp -d)
test -d "$p" && echo dir
### expect
dir
### end

### mktemp_dry_run
# -u/--dry-run prints a name without creating it
p=$(mktemp -u)
test -e "$p" || echo absent
### expect
absent
### end

### mktemp_template_xxxxxx
# Custom template: trailing X's are replaced with random chars
p=$(mktemp /tmp/myXXXXXX)
case "$p" in
  /tmp/my??????) echo ok ;;
  *) echo "bad: $p" ;;
esac
### expect
ok
### end

### mktemp_unknown_flag_rejected
### bash_diff: clap-backed mktemp returns exit 2 for parse errors; GNU mktemp returns 1
# Unknown long flag is a usage error
mktemp --no-such-flag 2>/dev/null; echo "exit=$?"
### expect
exit=2
### end

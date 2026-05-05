### readlink_canonicalize_f
# -f: canonicalize, all but last component must exist
mkdir -p /tmp/rl_f
touch /tmp/rl_f/file.txt
readlink -f /tmp/rl_f/file.txt
### expect
/tmp/rl_f/file.txt
### end

### readlink_canonicalize_missing_m
# -m: canonicalize without requiring components to exist
readlink -m /tmp/rl_missing/path
### expect
/tmp/rl_missing/path
### end

### readlink_canonicalize_existing_e
# -e: all components must exist
mkdir -p /tmp/rl_e
touch /tmp/rl_e/file.txt
readlink -e /tmp/rl_e/file.txt
### expect
/tmp/rl_e/file.txt
### end

### readlink_combined_short_flags
# Codegen-driven clap accepts combined short flags like -fn (-f + -n).
# We don't yet honor -n trailing-newline suppression, but parsing must
# not fail and -f's canonicalization must still apply.
mkdir -p /tmp/rl_fn
touch /tmp/rl_fn/file.txt
readlink -fn /tmp/rl_fn/file.txt
echo
### expect
/tmp/rl_fn/file.txt
### end

### readlink_long_form
# Long-form flag from generated args surface.
mkdir -p /tmp/rl_long
touch /tmp/rl_long/file.txt
readlink --canonicalize /tmp/rl_long/file.txt
### expect
/tmp/rl_long/file.txt
### end

### readlink_unknown_flag_rejected
# Clap rejects unknown flags with exit 2; GNU readlink exits 1.
# bash_diff captures the well-known clap-vs-GNU divergence.
### bash_diff
readlink --bogus /tmp/whatever
echo "exit=$?"
### expect
exit=2
### end

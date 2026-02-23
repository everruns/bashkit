### getopts_basic
# Parse simple options
OPTIND=1
while getopts "ab" opt -a -b; do
  echo "opt=$opt"
done
### expect
opt=a
opt=b
### end

### getopts_with_arg
# Parse option with argument
OPTIND=1
while getopts "f:" opt -f myfile; do
  echo "opt=$opt OPTARG=$OPTARG"
done
### expect
opt=f OPTARG=myfile
### end

### getopts_mixed
# Mix of options with and without args
OPTIND=1
while getopts "vf:o:" opt -v -f input -o output; do
  if [ -n "${OPTARG:-}" ]; then
    echo "$opt $OPTARG"
  else
    echo "$opt"
  fi
done
### expect
v
f input
o output
### end

### getopts_unknown
# Unknown option produces ?
OPTIND=1
getopts "ab" opt -x 2>/dev/null
echo "opt=$opt"
### expect
opt=?
### end

### getopts_no_more_options
# Returns 1 when no more options
OPTIND=1
### exit_code:1
getopts "a" opt hello
### expect
### end

### getopts_combined_flags
# Combined flags like -abc
OPTIND=1
while getopts "abc" opt -abc; do
  echo "$opt"
done
### expect
a
b
c
### end

### getopts_combined_with_arg
# Combined flag with trailing argument
OPTIND=1
while getopts "af:" opt -af myfile; do
  if [ -n "${OPTARG:-}" ]; then
    echo "$opt $OPTARG"
  else
    echo "$opt"
  fi
done
### expect
a
f myfile
### end

### getopts_silent_mode
# Silent mode (leading :) suppresses errors
OPTIND=1
getopts ":ab" opt -x 2>/dev/null
echo "opt=$opt OPTARG=$OPTARG"
### expect
opt=? OPTARG=x
### end

### getopts_double_dash
# -- stops option processing
OPTIND=1
getopts "a" opt -- -a
echo "exit=$?"
### expect
exit=1
### end

### getopts_local_optind_in_function
parse() {
  local OPTIND opt verbose=0 out=""
  while getopts ":vo:h" opt; do
    case $opt in
      v) verbose=1 ;;
      o) out=$OPTARG ;;
      :) echo "missing arg for -$OPTARG"; return 2 ;;
      \?) echo "invalid -$OPTARG"; return 2 ;;
    esac
  done
  shift $((OPTIND - 1))
  echo "verbose=$verbose out=$out rest=$*"
}
parse -v -o out.txt a b
parse -vo x
parse -x
parse -o
getopts "a" o -a; echo "top: $o OPTIND=$OPTIND"
### expect
verbose=1 out=out.txt rest=a b
verbose=1 out=x rest=
invalid -x
missing arg for -o
top: a OPTIND=2
### end

### getopts_optind_reset_restarts_group
getopts "ab" o -ab; echo "$o $OPTIND"
OPTIND=1
getopts "ab" o -ba; echo "$o $OPTIND"
### expect
a 1
b 1
### end

### getopts_local_optarg
f() { local OPTARG; getopts "x:" o -x val; echo "in: $OPTARG"; }
OPTARG=global; f; echo "out: $OPTARG"
### expect
in: val
out: global
### end

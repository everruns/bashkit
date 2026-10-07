### shift_count_errors
set -- a b
shift 3; echo "rc=$? n=$#"
shift abc 2>/dev/null; echo "rc=$? n=$#"
shift -1 2>/dev/null; echo "rc=$? n=$#"
shift 2; echo "rc=$? n=$#"
shift; echo "rc=$? n=$#"
### expect
rc=1 n=2
rc=1 n=2
rc=1 n=2
rc=0 n=0
rc=1 n=0
### end

### tilde_plus_minus
cd /tmp; cd /
echo ~+ ~- ~+/x ~-/y
### expect
/ /tmp //x /tmp/y
### end

### tilde_minus_unset_is_literal
unset OLDPWD
echo ~- ~unknownuser_zz
### expect
~- ~unknownuser_zz
### end

### shopt_o_set_options
shopt -o noglob
shopt -os noglob; shopt -o noglob; echo "rc=$?"
shopt -ou noglob
shopt -po errexit
shopt -oq pipefail; echo "q=$?"
set -o pipefail; shopt -oq pipefail; echo "q=$?"
shopt -o bogus 2>/dev/null; echo "bad=$?"
### expect
noglob         	off
noglob         	on
rc=0
set +o errexit
q=1
q=0
bad=1
### end

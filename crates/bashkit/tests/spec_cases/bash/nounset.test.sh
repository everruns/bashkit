### nounset_check_flag
# set -u sets SHOPT_u
### bash_diff: SHOPT_u is bashkit-internal variable
set -u
echo "SHOPT_u=$SHOPT_u"
### expect
SHOPT_u=1
### end

### nounset_unset_var_error
# set -u aborts on unset variables
### bash_diff: real bash prints to stderr and exits shell
### exit_code:1
set -u
echo $UNDEFINED_VAR_XYZ
echo "should not reach"
### expect
### end

### nounset_set_var_ok
# set -u allows set variables
set -u
MY_VAR=hello
echo "$MY_VAR"
### expect
hello
### end

### nounset_special_vars
# set -u allows special variables
set -u
echo "$?"
### expect
0
### end

### nounset_empty_var_ok
# set -u allows empty but set variables
set -u
EMPTY=""
echo "value=$EMPTY"
### expect
value=
### end

### nounset_default_value_ok
# ${var:-default} should not error under set -u
### skip: parameter expansion with :- needs nounset awareness
set -u
echo "${UNDEFINED_XYZ:-fallback}"
### expect
fallback
### end

### nounset_disable
# set +u disables nounset
set -u
set +u
echo "$UNDEFINED_VAR_XYZ"
echo "ok"
### expect

ok
### end

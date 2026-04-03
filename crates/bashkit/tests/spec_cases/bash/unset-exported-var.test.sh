### unset_exported_variable
# unset should remove exported vars completely
export UNSET_TEST=value
unset UNSET_TEST
echo "${UNSET_TEST:-gone}"
### expect
gone
### end

### unset_then_check_defined
### skip: [[ -v VAR ]] unary operator not yet implemented
# unset var should not be -v defined
export UNSET_DEF_TEST=x
unset UNSET_DEF_TEST
[[ -v UNSET_DEF_TEST ]] && echo "still set" || echo "unset"
### expect
unset
### end

### unset_regular_variable
# unset non-exported var (verify no regression)
REGULAR=hello
unset REGULAR
echo "${REGULAR:-gone}"
### expect
gone
### end

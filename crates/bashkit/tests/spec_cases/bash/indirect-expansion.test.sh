### indirect_expansion_with_default_operator
# ${!var:-} should resolve indirect then apply default
name="TARGET"; export TARGET="value"
echo "${!name:-fallback}"
### expect
value
### end

### indirect_expansion_with_default_unset
# ${!var:-default} when target is unset should return default
name="MISSING_VAR"
echo "${!name:-fallback}"
### expect
fallback
### end

### indirect_expansion_with_default_empty
# ${!var:-default} when target is empty should return default
name="EMPTY_VAR"; export EMPTY_VAR=""
echo "${!name:-fallback}"
### expect
fallback
### end

### indirect_expansion_with_assign_operator
# ${!var:=default} should also work with indirect
name="UNSET_TARGET"
echo "${!name:=assigned}"
### expect
assigned
### end

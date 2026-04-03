### conditional_and_short_circuit_set_u
# [[ false && unset_ref ]] should not evaluate right side
set -u
[[ -n "${UNSET_SC_VAR:-}" && "${UNSET_SC_VAR}" != "off" ]]
echo $?
### expect
1
### end

### conditional_or_short_circuit_set_u
# [[ true || unset_ref ]] should not evaluate right side
set -u
[[ -z "${UNSET_SC_VAR2:-}" || "${UNSET_SC_VAR2}" == "x" ]]
echo $?
### expect
0
### end

### conditional_and_short_circuit_passes
# [[ true && check ]] should evaluate both sides
set -u
export SC_SET_VAR="active"
[[ -n "${SC_SET_VAR:-}" && "${SC_SET_VAR}" != "off" ]]
echo $?
### expect
0
### end

### conditional_nested_short_circuit
# Nested && || should respect short-circuit
set -u
[[ -n "${UNSET_NSC:-}" && "${UNSET_NSC}" == "x" ]] || echo "safe"
### expect
safe
### end

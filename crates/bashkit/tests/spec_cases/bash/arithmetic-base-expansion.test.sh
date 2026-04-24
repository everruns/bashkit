### arithmetic_base_with_suffix_removal
# 10#${var%%-*} should expand then convert
last="0003-assistant.md"
echo $(( 10#${last%%-*} ))
### expect
3
### end

### arithmetic_base_with_prefix_removal
# 10#${var##0} should expand then convert
val="007"
echo $(( 10#${val##0} ))
### expect
7
### end

### arithmetic_base_with_expansion_plus
# 10#${var%%-*} + 1 in arithmetic
seq="0041-user.md"
echo $(( 10#${seq%%-*} + 1 ))
### expect
42
### end

### arithmetic_base_simple_var
# 10#${var} without operators (verify no regression)
x="0099"
echo $(( 10#${x} ))
### expect
99
### end


### arithmetic_base_suffix_pattern_contains_double_percent
# Ensure %% inside pattern is not treated as operator location
var="123foo%%bar"
echo $(( 10#${var%foo%%bar} ))
### expect
123
### end

### arithmetic_base_prefix_pattern_contains_double_hash
# Ensure ## inside pattern is not treated as operator location
var="foo##bar123"
echo $(( 10#${var#foo##bar} ))
### expect
123
### end
